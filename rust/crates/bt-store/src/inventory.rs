// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/inventory.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/model/StockPurchaseBase.h and src/model/IngredientBase.h

//! How much of each ingredient is on hand.
//!
//! An ingredient is not bought once: it is bought in lots, and each lot is drawn down by the
//! brews that use it.  What is left of a lot is what was received less everything taken from it,
//! and what is on hand is the sum of what is left of every lot.
//!
//! Ported from `StockPurchaseBase::getTotalInventory` and `StockPurchaseBase::quantityRemaining`.
//! The C++ keeps a running figure on each use as it goes and reads the last one; the sum is the
//! same number and does not need the uses to be in order.

use std::collections::BTreeMap;

use bt_model::recipe::IngredientAmount;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{Store, StoreError, tables};

/// Which kind of ingredient a holding is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Fermentable,
    Hop,
    Misc,
    Yeast,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::Fermentable, Self::Hop, Self::Misc, Self::Yeast];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fermentable => "fermentable",
            Self::Hop => "hop",
            Self::Misc => "misc",
            Self::Yeast => "yeast",
        }
    }
}

impl std::str::FromStr for Kind {
    type Err = StoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| StoreError::UnknownTable(value.to_owned()))
    }
}

/// What is left of one lot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lot {
    pub purchase_id: i32,
    /// What was received, in the canonical unit.
    pub received: f64,
    /// What has been taken from this lot since.
    pub used: f64,
    /// What is left of it.
    pub remaining: f64,
    pub supplier: Option<String>,
    pub date_received: Option<NaiveDate>,
    pub date_best_before: Option<NaiveDate>,
}

/// How much of one ingredient is on hand, and which lots it is in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Holding {
    pub kind: Kind,
    pub ingredient_id: i32,
    pub ingredient_name: Option<String>,
    /// The total still available, in the canonical unit for whatever it is measured in --
    /// kilograms for everything that can be an ingredient of a recipe.
    pub quantity: f64,
    pub unit: &'static str,
    /// Whether any is left.  `IngredientBase::isOnHand`.
    pub on_hand: bool,
    pub lots: Vec<Lot>,
}

/// A quantity close enough to nothing to count as nothing.
///
/// `qFuzzyIsNull`, which is what the C++ uses to decide whether a lot is spent.
fn is_nothing(quantity: f64) -> bool {
    quantity.abs() < 1e-12
}

/// Work out the holdings of one kind of ingredient.
///
/// The four kinds differ only in the names of their tables and the columns that join them, so one
/// macro covers all four rather than four copies that could drift apart.
macro_rules! holdings_of {
    ($name:ident, $kind:expr, $ingredients:ident, $purchases:ident, $uses:ident, $ingredient_key:ident, $purchase_key:ident) => {
        async fn $name(store: &Store) -> Result<Vec<Holding>, StoreError> {
            let purchases = tables::$purchases::list(store).await?;
            let uses = tables::$uses::list(store).await?;
            let ingredients = tables::$ingredients::list(store).await?;

            // What has been taken from each lot.  A use whose lot was deleted is ignored, which
            // falls out of looking the lot up rather than needing a check of its own.
            let mut used: BTreeMap<i32, f64> = BTreeMap::new();
            for change in uses.iter().filter(|change| !change.deleted.unwrap_or(false)) {
                if let Some(purchase_id) = change.$purchase_key {
                    *used.entry(purchase_id).or_default() += change.quantity_used.unwrap_or(0.0);
                }
            }

            let mut holdings: BTreeMap<i32, Holding> = BTreeMap::new();
            for purchase in purchases.iter().filter(|purchase| !purchase.deleted.unwrap_or(false)) {
                let Some(ingredient_id) = purchase.$ingredient_key else {
                    // A lot of nothing in particular cannot be counted against anything.
                    continue;
                };
                let received = purchase.canonical_amount().map(|amount| amount.quantity).unwrap_or(0.0);
                let used = used.get(&purchase.id).copied().unwrap_or(0.0);
                let remaining = received - used;

                let holding = holdings.entry(ingredient_id).or_insert_with(|| Holding {
                    kind: $kind,
                    ingredient_id,
                    ingredient_name: None,
                    quantity: 0.0,
                    unit: UNIT,
                    on_hand: false,
                    lots: Vec::new(),
                });
                holding.lots.push(Lot {
                    purchase_id: purchase.id,
                    received,
                    used,
                    remaining,
                    supplier: purchase.supplier.clone(),
                    date_received: purchase.date_received,
                    date_best_before: purchase.date_best_before,
                });
                // A spent lot is left out of the total, as `getTotalInventory` leaves it out of
                // the purchases it sums -- it is still listed, because knowing a lot is empty is
                // worth more than not knowing it exists.
                if !is_nothing(remaining) {
                    holding.quantity += remaining;
                }
            }

            for ingredient in &ingredients {
                if let Some(holding) = holdings.get_mut(&ingredient.id) {
                    holding.ingredient_name = ingredient.name.clone();
                }
            }
            Ok(holdings
                .into_values()
                .map(|holding| Holding { on_hand: !is_nothing(holding.quantity), ..holding })
                .collect())
        }
    };
}

/// Everything that can be an ingredient of a recipe is weighed, so every total is in kilograms.
const UNIT: &str = "kilograms";

holdings_of!(
    fermentable_holdings,
    Kind::Fermentable,
    fermentable,
    fermentable_stock_purchase,
    fermentable_stock_use,
    fermentable_id,
    fermentable_purchase_id
);
holdings_of!(hop_holdings, Kind::Hop, hop, hop_stock_purchase, hop_stock_use, hop_id, hop_purchase_id);
holdings_of!(misc_holdings, Kind::Misc, misc, misc_stock_purchase, misc_stock_use, misc_id, misc_purchase_id);
holdings_of!(yeast_holdings, Kind::Yeast, yeast, yeast_stock_purchase, yeast_stock_use, yeast_id, yeast_purchase_id);

/// What is on hand of one kind of ingredient.
pub async fn holdings(store: &Store, kind: Kind) -> Result<Vec<Holding>, StoreError> {
    match kind {
        Kind::Fermentable => fermentable_holdings(store).await,
        Kind::Hop => hop_holdings(store).await,
        Kind::Misc => misc_holdings(store).await,
        Kind::Yeast => yeast_holdings(store).await,
    }
}

/// What is on hand, of everything.
pub async fn everything(store: &Store) -> Result<Vec<Holding>, StoreError> {
    let mut all = Vec::new();
    for kind in Kind::ALL {
        all.extend(holdings(store, kind).await?);
    }
    Ok(all)
}
