// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/tools.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/{ogCorrectionTool,refractoDialog,pitchDialog,primingDialog,strikeWaterDialog}.ui

//! The small tools: five calculators that answer one question each.
//!
//! In the Qt application these are dialogs off the Tools menu.  Here they are screens, since a
//! dialog is a desktop idiom and an address is a better one on the web -- a brewer can keep the
//! refractometer tool open on a phone beside the kettle.
//!
//! Each works the same way: type numbers in, read the answer out.  Nothing is stored, so there
//! is nothing to save and no way to lose anything.

use dioxus::prelude::*;

use super::format::NOTHING;

/// Which tool is being used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    StrikeWater,
    Refractometer,
    HydrometerCorrection,
    Priming,
    PitchRate,
}

impl Tool {
    pub const ALL: [Self; 5] =
        [Self::StrikeWater, Self::Refractometer, Self::HydrometerCorrection, Self::Priming, Self::PitchRate];

    pub fn title(self) -> &'static str {
        match self {
            Self::StrikeWater => "Strike Water",
            Self::Refractometer => "Refractometer",
            Self::HydrometerCorrection => "Hydrometer Correction",
            Self::Priming => "Priming",
            Self::PitchRate => "Pitch Rate",
        }
    }

    /// What this tool is called in an address.
    pub fn slug(self) -> &'static str {
        match self {
            Self::StrikeWater => "strike-water",
            Self::Refractometer => "refractometer",
            Self::HydrometerCorrection => "hydrometer",
            Self::Priming => "priming",
            Self::PitchRate => "pitch-rate",
        }
    }
}

impl std::fmt::Display for Tool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.slug())
    }
}

/// An address naming a tool that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("there is no tool called {0:?}")]
pub struct NoSuchTool(String);

impl std::str::FromStr for Tool {
    type Err = NoSuchTool;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|tool| tool.slug() == value).ok_or_else(|| NoSuchTool(value.to_owned()))
    }
}

/// A number typed into a tool.
///
/// Kept as text rather than as a number so that a half-typed "1." or an empty box is a state
/// the screen can be in, rather than something that has to be rejected as it is typed.
#[derive(Clone, Copy, PartialEq)]
struct Field(Signal<String>);

impl Field {
    fn value(&self) -> Option<f64> {
        self.0.read().trim().parse().ok()
    }
}

/// One labelled input.
#[component]
fn Input(label: &'static str, field: Field, #[props(default)] suffix: &'static str) -> Element {
    let id = format!("tool-{}", label.replace(' ', "-").to_lowercase());
    let mut value = field.0;
    rsx! {
        div { class: "bt-field",
            label { r#for: "{id}", "{label}" }
            div { class: "bt-input-row",
                input {
                    id: "{id}",
                    r#type: "number",
                    step: "any",
                    value: "{value}",
                    oninput: move |event| value.set(event.value()),
                }
                if !suffix.is_empty() {
                    span { class: "bt-suffix", "{suffix}" }
                }
            }
        }
    }
}

/// One answer.
#[component]
fn Answer(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "bt-figure",
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}

/// Format an answer, or a dash where there is not enough to work one out.
fn answer(value: Option<f64>, precision: usize, suffix: &str) -> String {
    match value {
        Some(value) if value.is_finite() => format!("{value:.precision$}{suffix}"),
        _ => NOTHING.to_owned(),
    }
}

/// Whichever tool the address names.
#[component]
pub fn Tools(tool: Tool) -> Element {
    rsx! {
        h2 { "{tool.title()}" }
        div { class: "bt-tool",
            match tool {
                Tool::StrikeWater => rsx! { StrikeWater {} },
                Tool::Refractometer => rsx! { Refractometer {} },
                Tool::HydrometerCorrection => rsx! { HydrometerCorrection {} },
                Tool::Priming => rsx! { Priming {} },
                Tool::PitchRate => rsx! { PitchRate {} },
            }
        }
    }
}

/// How hot the strike water has to be, and how much to add for a step infusion.
#[component]
fn StrikeWater() -> Element {
    let grain_temp = Field(use_signal(|| "20".to_owned()));
    let target = Field(use_signal(|| "66".to_owned()));
    let thickness = Field(use_signal(|| "3".to_owned()));

    let mash_from = Field(use_signal(|| "66".to_owned()));
    let mash_to = Field(use_signal(|| "72".to_owned()));
    let grain_kg = Field(use_signal(|| "5".to_owned()));
    let water_temp = Field(use_signal(|| "100".to_owned()));
    let mash_volume = Field(use_signal(|| "16".to_owned()));

    let strike = match (grain_temp.value(), target.value(), thickness.value()) {
        (Some(grain), Some(target), Some(thickness)) => bt_calc::tools::strike_water_temp_c(grain, target, thickness),
        _ => None,
    };
    let infusion = match (mash_from.value(), mash_to.value(), grain_kg.value(), water_temp.value(), mash_volume.value())
    {
        (Some(from), Some(to), Some(grain), Some(water), Some(volume)) => {
            bt_calc::tools::mash_infusion_l(from, to, grain, water, volume)
        }
        _ => None,
    };

    rsx! {
        h3 { class: "bt-group", "Initial Infusion" }
        div { class: "bt-fields",
            Input { label: "Original Grain Temperature", field: grain_temp, suffix: "°C" }
            Input { label: "Target Mash Temperature", field: target, suffix: "°C" }
            Input { label: "Water to Grain", field: thickness, suffix: "L/kg" }
        }
        dl { class: "bt-derived",
            Answer { label: "Strike Water Temperature", value: answer(strike, 1, " °C") }
        }

        h3 { class: "bt-group", "Mash Infusion" }
        div { class: "bt-fields",
            Input { label: "Actual Mash Temperature", field: mash_from, suffix: "°C" }
            Input { label: "Target Mash Temperature", field: mash_to, suffix: "°C" }
            Input { label: "Grain Weight", field: grain_kg, suffix: "kg" }
            Input { label: "Infusion Water Temperature", field: water_temp, suffix: "°C" }
            Input { label: "Total Volume of Water", field: mash_volume, suffix: "L" }
        }
        dl { class: "bt-derived",
            Answer { label: "Volume to Add", value: answer(infusion, 2, " L") }
        }
    }
}

/// What a refractometer reading means once there is alcohol in the sample.
#[component]
fn Refractometer() -> Element {
    let original_plato = Field(use_signal(|| "12".to_owned()));
    let current_plato = Field(use_signal(|| "6".to_owned()));

    let (og, sg, abv, abw, index, real_extract) = match (original_plato.value(), current_plato.value()) {
        (Some(original), Some(current)) => {
            let og = bt_calc::algorithms::plato_to_sg_20c20c(original);
            let sg = bt_calc::algorithms::sg_by_starting_plato(original, current);
            (
                Some(og),
                Some(sg),
                Some(bt_calc::algorithms::abv_by_sg_plato(sg, original)),
                Some(bt_calc::algorithms::abw_by_sg_plato(sg, original)),
                Some(bt_calc::algorithms::refractive_index(original)),
                Some(bt_calc::algorithms::real_extract(sg, original)),
            )
        }
        _ => (None, None, None, None, None, None),
    };

    rsx! {
        p { class: "bt-note",
            "Alcohol bends light differently from sugar, so a refractometer over-reads once
             fermentation has started.  Given what it read before and now, this works out what the
             beer is actually doing."
        }
        div { class: "bt-fields",
            Input { label: "Original Plato", field: original_plato, suffix: "°P" }
            Input { label: "Current Plato", field: current_plato, suffix: "°P" }
        }
        dl { class: "bt-derived",
            Answer { label: "OG (20 °C)", value: answer(og, 3, "") }
            Answer { label: "SG (20 °C)", value: answer(sg, 3, "") }
            Answer { label: "ABV", value: answer(abv, 1, "%") }
            Answer { label: "ABW", value: answer(abw, 1, "%") }
            Answer { label: "Refractive Index", value: answer(index, 4, "") }
            Answer { label: "Real Extract", value: answer(real_extract, 2, " °P") }
        }
    }
}

/// What a hydrometer reading means at a temperature it was not calibrated for.
#[component]
fn HydrometerCorrection() -> Element {
    let measured = Field(use_signal(|| "1.050".to_owned()));
    let reading_temp = Field(use_signal(|| "30".to_owned()));
    let calibration_temp = Field(use_signal(|| "20".to_owned()));

    let actual = match (measured.value(), reading_temp.value(), calibration_temp.value()) {
        (Some(measured), Some(reading), Some(calibration)) => {
            Some(bt_calc::algorithms::correct_sg_for_temperature(measured, reading, calibration))
        }
        _ => None,
    };

    rsx! {
        p { class: "bt-note",
            "Wort is less dense when it is warm, so a hydrometer reads low in hot wort and high in
             cold.  A hydrometer is calibrated for one temperature; this corrects a reading taken
             at another."
        }
        div { class: "bt-fields",
            Input { label: "Measured Gravity", field: measured }
            Input { label: "At Temperature", field: reading_temp, suffix: "°C" }
            Input { label: "Calibrated At", field: calibration_temp, suffix: "°C" }
        }
        dl { class: "bt-derived",
            Answer { label: "Actual SG", value: answer(actual, 4, "") }
        }
    }
}

/// How much sugar to prime with.
#[component]
fn Priming() -> Element {
    use bt_calc::tools::PrimingSugar;

    let beer_l = Field(use_signal(|| "19".to_owned()));
    let temp_c = Field(use_signal(|| "20".to_owned()));
    let vols = Field(use_signal(|| "2.4".to_owned()));
    let mut sugar = use_signal(|| PrimingSugar::Sucrose);

    let sugars = [
        (PrimingSugar::GlucoseMonohydrate, "Glucose Monohydrate (corn sugar)"),
        (PrimingSugar::Glucose, "Anhydrous Glucose"),
        (PrimingSugar::Sucrose, "Sucrose (table sugar)"),
        (PrimingSugar::DryMaltExtract, "Dry Malt Extract"),
    ];

    let grams = match (beer_l.value(), temp_c.value(), vols.value()) {
        (Some(beer), Some(temp), Some(vols)) => Some(bt_calc::tools::priming_sugar_g(beer, temp, vols, sugar())),
        _ => None,
    };
    let residual = temp_c.value().map(bt_calc::tools::residual_carbonation_vols);

    rsx! {
        div { class: "bt-fields",
            Input { label: "Collected Beer Volume", field: beer_l, suffix: "L" }
            Input { label: "Beer Temperature", field: temp_c, suffix: "°C" }
            Input { label: "Desired Volumes", field: vols, suffix: "vols" }
            div { class: "bt-field",
                label { "Prime With" }
                select {
                    value: "{sugar():?}",
                    onchange: move |event| {
                        if let Some((chosen, _)) = sugars.iter().find(|(kind, _)| format!("{kind:?}") == event.value()) {
                            sugar.set(*chosen);
                        }
                    },
                    for (kind, name) in sugars {
                        option { key: "{kind:?}", value: "{kind:?}", "{name}" }
                    }
                }
            }
        }
        dl { class: "bt-derived",
            Answer { label: "Sugar Needed", value: answer(grams, 1, " g") }
            Answer { label: "Already Dissolved", value: answer(residual, 2, " vols") }
        }
        if grams == Some(0.0) {
            p { class: "bt-note",
                "The beer already holds at least as much carbon dioxide as asked for, so there is
                 nothing to add.  Conditioning it colder would hold more in."
            }
        }
    }
}

/// How much yeast to pitch.
#[component]
fn PitchRate() -> Element {
    let volume_l = Field(use_signal(|| "20".to_owned()));
    let plato = Field(use_signal(|| "12".to_owned()));
    let rate = Field(use_signal(|| "0.75".to_owned()));
    let viability = Field(use_signal(|| "100".to_owned()));

    let pitch = match (volume_l.value(), plato.value(), rate.value(), viability.value()) {
        (Some(volume), Some(plato), Some(rate), Some(viability)) => {
            // A fresh pack is about 100 billion cells; viability takes a share off that.
            Some(bt_calc::tools::pitch(volume, plato, rate, 100.0 * viability / 100.0))
        }
        _ => None,
    };

    rsx! {
        p { class: "bt-note",
            "A typical ale is pitched at about 0.75 million cells per millilitre per degree Plato,
             and a lager at about twice that."
        }
        div { class: "bt-fields",
            Input { label: "Wort Volume", field: volume_l, suffix: "L" }
            Input { label: "Original Gravity", field: plato, suffix: "°P" }
            Input { label: "Pitch Rate", field: rate, suffix: "M/(mL·°P)" }
            Input { label: "Yeast Viability", field: viability, suffix: "%" }
        }
        dl { class: "bt-derived",
            Answer { label: "Cells Required", value: answer(pitch.map(|p| p.cells_billions), 0, " billion") }
            Answer { label: "Vials / Smack Packs", value: answer(pitch.map(|p| p.vials), 1, "") }
            Answer { label: "Dry Yeast", value: answer(pitch.map(|p| p.dry_grams), 1, " g") }
        }
    }
}
