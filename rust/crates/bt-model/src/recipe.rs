// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-model/src/recipe.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! A recipe and everything hanging off it.
//!
//! Ported from `src/model/Recipe.cpp` and the `RecipeAddition*` classes.
//!
//! A `recipe` row on its own says very little: the hops, fermentables, miscs and yeasts live in
//! their own tables, and the mash, boil and fermentation are separate objects the recipe points
//! at.  Nothing can be calculated from the row alone, so the aggregate here is what the
//! calculations are actually given.
//!
//! Note what is *not* here: an addition holds its own quantity and unit and refers to the
//! ingredient, rather than copying it.  Two recipes using the same hop share that hop's row, so
//! correcting its alpha acid corrects both.

use bt_measure::{Amount, Unit};

use crate::enums::{RecipeAdditionStage, UnknownValue};
use crate::tables::*;

/// One ingredient added to a recipe, with the ingredient it refers to.
///
/// Ported from `RecipeAdditionBase`.  The `addition` row carries how much and when; the
/// `ingredient` row carries what it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Addition<Row, Ingredient> {
    pub addition: Row,
    /// `None` when the ingredient row is missing, which a foreign key should prevent but a
    /// hand-edited database may not.
    pub ingredient: Option<Ingredient>,
}

/// A recipe with everything needed to calculate from it.
///
/// Ported from the object graph `Recipe` assembles in the C++.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Recipe {
    pub row: RecipeRow,
    pub fermentables: Vec<Addition<FermentableInRecipeRow, FermentableRow>>,
    pub hops: Vec<Addition<HopInRecipeRow, HopRow>>,
    pub miscs: Vec<Addition<MiscInRecipeRow, MiscRow>>,
    pub yeasts: Vec<Addition<YeastInRecipeRow, YeastRow>>,
    pub equipment: Option<EquipmentRow>,
    pub style: Option<StyleRow>,
    /// The mash and its steps, in step order.
    pub mash: Option<(MashRow, Vec<MashStepRow>)>,
    pub boil: Option<(BoilRow, Vec<BoilStepRow>)>,
    pub fermentation: Option<(FermentationRow, Vec<FermentationStepRow>)>,
    pub instructions: Vec<InstructionRow>,
}

impl Recipe {
    pub fn id(&self) -> i32 {
        self.row.id
    }

    pub fn name(&self) -> &str {
        self.row.name.as_deref().unwrap_or_default()
    }

    /// Whether the recipe is locked against editing.
    ///
    /// Ported from `LockablePropertyBase`.  A locked recipe is one that has been brewed and is
    /// being kept as a record, so editing it would rewrite history.
    pub fn is_locked(&self) -> bool {
        self.row.locked.unwrap_or(false)
    }

    pub fn is_deleted(&self) -> bool {
        self.row.deleted.unwrap_or(false)
    }

    /// The recipe this one was derived from, if any.
    ///
    /// A recipe is its own ancestor when it has never been superseded, which is what
    /// `migrate_to_10` set up, so that case reads as "no ancestor" here.
    pub fn ancestor_id(&self) -> Option<i32> {
        self.row.ancestor_id.filter(|ancestor| *ancestor != self.row.id)
    }

    /// Every addition that is not marked deleted, by stage.
    pub fn hops_at(&self, stage: RecipeAdditionStage) -> impl Iterator<Item = &Addition<HopInRecipeRow, HopRow>> {
        self.hops.iter().filter(move |addition| {
            !addition.addition.deleted.unwrap_or(false) && addition.addition.stage().ok().flatten() == Some(stage)
        })
    }

    pub fn fermentables_at(
        &self,
        stage: RecipeAdditionStage,
    ) -> impl Iterator<Item = &Addition<FermentableInRecipeRow, FermentableRow>> {
        self.fermentables.iter().filter(move |addition| {
            !addition.addition.deleted.unwrap_or(false) && addition.addition.stage().ok().flatten() == Some(stage)
        })
    }

    /// The mash steps in the order they are carried out.
    pub fn mash_steps(&self) -> &[MashStepRow] {
        self.mash.as_ref().map_or(&[], |(_, steps)| steps.as_slice())
    }

    pub fn boil_steps(&self) -> &[BoilStepRow] {
        self.boil.as_ref().map_or(&[], |(_, steps)| steps.as_slice())
    }

    pub fn fermentation_steps(&self) -> &[FermentationStepRow] {
        self.fermentation.as_ref().map_or(&[], |(_, steps)| steps.as_slice())
    }
}

/// How much of an ingredient an addition calls for.
///
/// The four addition tables share these columns, inherited from `INGREDIENT_AMOUNT_COMMON_FIELDS`
/// in `ObjectStoreTyped.cpp`.
pub trait IngredientAmount {
    /// What the ingredient this is an addition of is measured in -- mass for a hop, volume for
    /// water.  `defaultMeasure` in each model class: `Fermentable.h`, `Hop.h`, `Misc.h`,
    /// `Yeast.h`, `Water.h`.
    const DEFAULT_MEASURE: &'static str;

    fn quantity(&self) -> Option<f64>;
    fn unit(&self) -> Option<Unit>;

    /// The amount as given.
    ///
    /// A row that records how much but not in what is read in the canonical unit for whatever the
    /// ingredient is measured in, which is what `IngredientAmount`'s constructor in
    /// `src/model/IngredientAmount.h` does when the bundle has a quantity and no unit.  BeerXML
    /// arrives that way: it has an `AMOUNT` and specifies that it is in kilograms or litres,
    /// with no element to say which.
    fn amount(&self) -> Option<Amount> {
        let unit = match self.unit() {
            Some(unit) => unit,
            None => Unit::canonical_for(Self::DEFAULT_MEASURE)?,
        };
        Some(Amount { quantity: self.quantity()?, unit })
    }

    /// The amount converted to the canonical unit for whatever it measures: kilograms for a mass,
    /// litres for a volume.  Values are held canonically and converted at the edges, as
    /// `src/measurement/` does.
    fn canonical_amount(&self) -> Option<Amount> {
        let amount = self.amount()?;
        Some(amount.unit.to_canonical(amount.quantity))
    }
}

macro_rules! ingredient_amount {
    ($($row:ty => $measure:literal),+ $(,)?) => {
        $(
            impl IngredientAmount for $row {
                const DEFAULT_MEASURE: &'static str = $measure;

                fn quantity(&self) -> Option<f64> {
                    self.quantity
                }

                fn unit(&self) -> Option<Unit> {
                    <$row>::unit(self)
                }
            }
        )+
    };
}

// Every ingredient that can be added to a recipe is weighed; only water is measured by volume,
// and water is not an addition.
ingredient_amount!(
    FermentableInRecipeRow => "Mass",
    HopInRecipeRow => "Mass",
    MiscInRecipeRow => "Mass",
    YeastInRecipeRow => "Mass",
    FermentableStockPurchaseRow => "Mass",
    HopStockPurchaseRow => "Mass",
    MiscStockPurchaseRow => "Mass",
    YeastStockPurchaseRow => "Mass",
);

/// The stage an addition belongs to, surfacing a value the database holds but this version does
/// not recognise rather than silently dropping the addition.
pub fn stage_of<Row>(addition: &Addition<Row, impl Sized>) -> Result<Option<RecipeAdditionStage>, UnknownValue>
where
    Row: HasStage,
{
    addition.addition.stage_value()
}

/// Implemented by the addition rows, all of which carry a `stage` column.
pub trait HasStage {
    fn stage_value(&self) -> Result<Option<RecipeAdditionStage>, UnknownValue>;
}

macro_rules! has_stage {
    ($($row:ty),+ $(,)?) => {
        $(
            impl HasStage for $row {
                fn stage_value(&self) -> Result<Option<RecipeAdditionStage>, UnknownValue> {
                    <$row>::stage(self)
                }
            }
        )+
    };
}

has_stage!(FermentableInRecipeRow, HopInRecipeRow, MiscInRecipeRow, YeastInRecipeRow);
