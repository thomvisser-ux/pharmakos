// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Combat's rows as the tick reads them: every weapon, the rules of
//! engagement's window, the materials' blast levels and the bedrock layer
//! (S2's plan, task `fire`; decisions 4, 10 and 17; decisions-log item 145
//! (5)).
//!
//! `con3` wrote the rows and no reader; this module is the reader, and every
//! value a tick could not use is **refused at load** with a typed error rather
//! than read short (the operator's global rules; S2's plan, E7): a zero
//! `cooldown_ms` (a weapon that fires every tick without bound), a zero
//! projectile `speed_voxels_per_second` (a shell that never lands), a zero
//! `range_voxels` (a weapon that reaches nothing), a zero `damage_hp` (a shot
//! that removes nothing, which [`Damage`] cannot hold), an under-attack window
//! of no time (`RETURN_FIRE` would never fire), a salvage percent past 100, a
//! blast level outside
//! one to three (no material to crack into), and zero bedrock layers (the
//! support rule needs a floor).

use crate::math::fixed::Fx;
use crate::math::quantity::{Damage, Ms, TICK_HZ};
use crate::rules::RulesError;
use crate::voxels::BlastLevels;
use pharmakos_proto::gp;

/// An indirect weapon's shell, as the tick reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProjectileRow {
    /// Flight speed in whole voxels per second of game time: never zero.
    pub speed_voxels_per_second: u32,
    /// Every asset within this radius of the landing point takes the weapon's
    /// full damage (no falloff, decision 17), in whole voxels.
    pub splash_radius_voxels: i16,
    /// The blast the landing files: a ball of this radius, each voxel stepped
    /// down one level (decision 10), in whole voxels.
    pub crater_radius_voxels: i16,
}

impl ProjectileRow {
    /// The splash radius in Q16.16 voxels.
    #[must_use]
    pub const fn splash(self) -> Fx {
        Fx::from_voxels(self.splash_radius_voxels)
    }

    /// Whole ticks a shell takes to fly `distance` (Q16.16 voxels), rounded
    /// **up**, and at least one: a shell lands on a later tick than it is
    /// fired, never on its own (S2's plan, E13).
    ///
    /// `ceil(distance * TICK_HZ / speed)`, in integers over the raw Q16.16
    /// value. `None` past a `u32` of ticks, which no map reaches.
    #[must_use]
    pub fn flight_ticks(self, distance: Fx) -> Option<u32> {
        let raw = u128::try_from(distance.raw()).ok()?;
        let per_tick = u128::from(self.speed_voxels_per_second).checked_mul(1 << 16)?;
        let numerator = raw.checked_mul(u128::from(TICK_HZ))?;
        let ticks = numerator
            .checked_add(per_tick.checked_sub(1)?)?
            .checked_div(per_tick)?;
        u32::try_from(ticks.max(1)).ok()
    }
}

/// One weapon as the tick reads it ([`gp::v1::rules_table::Weapon`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WeaponRow {
    /// How far it reaches, in whole voxels: a target is in range when its
    /// squared distance is no more than this squared (AGENTS.md section 4.2).
    pub range_voxels: i16,
    /// Hit points one shot takes, at least one.
    pub damage: Damage,
    /// Game milliseconds between two shots, positive.
    pub cooldown_ms: Ms,
    /// The shell, for an indirect weapon; `None` for direct fire, whose damage
    /// lands in the tick it is dealt (S2's plan, E2).
    pub projectile: Option<ProjectileRow>,
}

/// An armed asset's threat (S2's plan, decision 12): its weapon's damage per
/// second, from its row, in **thousandths of a hit point per second** so the
/// rate is an integer without rounding a 250 ms cooldown away. Zero for an
/// unarmed asset. A pure function of the row, which `dfnd` and `atk` read.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Threat(u64);

impl Threat {
    /// No threat: an unarmed asset.
    pub const NONE: Threat = Threat(0);

    /// The threat in thousandths of a hit point per second.
    #[must_use]
    pub const fn milli_hp_per_second(self) -> u64 {
        self.0
    }

    /// The threat of an asset carrying `weapon`, or [`Threat::NONE`] for none.
    /// `damage * 1 000 000 / cooldown_ms`, rounded down: the autocannon's
    /// 12 HP every 250 ms is 48 000, 48 HP a second.
    #[must_use]
    pub fn of(weapon: Option<WeaponRow>) -> Threat {
        let Some(weapon) = weapon else {
            return Threat::NONE;
        };
        // A loaded weapon's damage and cooldown are both positive, so the
        // quotient exists and fits: at most `i32::MAX * 10^6`.
        let damage = u64::from(weapon.damage.raw().unsigned_abs());
        let cooldown = u64::from(weapon.cooldown_ms.raw().unsigned_abs());
        damage
            .checked_mul(1_000_000)
            .and_then(|scaled| scaled.checked_div(cooldown))
            .map_or(Threat::NONE, Threat)
    }
}

impl WeaponRow {
    /// The range in Q16.16 voxels.
    #[must_use]
    pub const fn range(self) -> Fx {
        Fx::from_voxels(self.range_voxels)
    }

    /// The cooldown in whole ticks, rounded up and at least one, so a weapon
    /// never fires twice in a tick and never fires faster than its row.
    #[must_use]
    pub fn cooldown_ticks(self) -> u32 {
        self.cooldown_ms.to_ticks_ceil().max(1)
    }

    /// The longest flight a shell of this weapon can take: one aimed at its
    /// full range. `None` for direct fire.
    #[must_use]
    pub fn longest_flight_ticks(self) -> Option<u32> {
        self.projectile
            .and_then(|shell| shell.flight_ticks(self.range()))
    }

    /// Read one weapon row, `field` naming it for the error.
    ///
    /// # Errors
    ///
    /// [`RulesError::OutOfRange`] for a zero or past-a-coordinate range, a
    /// zero or past-`i32` damage, a cooldown that is not positive, and a
    /// projectile whose speed is zero or whose radii are not voxel lengths.
    pub fn read(
        field: &'static str,
        row: &gp::v1::rules_table::Weapon,
    ) -> Result<WeaponRow, RulesError> {
        let refused = |what: &str, value: String| RulesError::OutOfRange {
            field: format!("{field}.{what}"),
            value,
        };
        if row.range_voxels == 0 {
            return Err(refused(
                "range_voxels",
                "0; a weapon that reaches nothing never fires".to_owned(),
            ));
        }
        let range_voxels = i16::try_from(row.range_voxels).map_err(|_| {
            refused(
                "range_voxels",
                format!("{}; a range is a voxel length", row.range_voxels),
            )
        })?;
        let damage = Damage::new(i64::from(row.damage_hp)).map_err(|_| {
            refused(
                "damage_hp",
                format!(
                    "{}; a shot removes one to {} hit points",
                    row.damage_hp,
                    i32::MAX
                ),
            )
        })?;
        if row.cooldown_ms <= 0 {
            return Err(refused(
                "cooldown_ms",
                format!(
                    "{}; a weapon with no cooldown fires every tick without bound",
                    row.cooldown_ms
                ),
            ));
        }
        let projectile = match row.projectile.as_ref() {
            None => None,
            Some(shell) => {
                if shell.speed_voxels_per_second == 0 {
                    return Err(refused(
                        "projectile.speed_voxels_per_second",
                        "0; a shell that does not fly never lands".to_owned(),
                    ));
                }
                let radius = |what: &'static str, value: u32| {
                    i16::try_from(value)
                        .map_err(|_| refused(what, format!("{value}; a radius is a voxel length")))
                };
                Some(ProjectileRow {
                    speed_voxels_per_second: shell.speed_voxels_per_second,
                    splash_radius_voxels: radius(
                        "projectile.splash_radius_voxels",
                        shell.splash_radius_voxels,
                    )?,
                    crater_radius_voxels: radius(
                        "projectile.crater_radius_voxels",
                        shell.crater_radius_voxels,
                    )?,
                })
            }
        };
        Ok(WeaponRow {
            range_voxels,
            damage,
            cooldown_ms: Ms::new(row.cooldown_ms),
            projectile,
        })
    }

    /// Read an optional weapon row: absent is an unarmed kind.
    ///
    /// # Errors
    ///
    /// As [`WeaponRow::read`].
    pub fn read_optional(
        field: &'static str,
        row: Option<&gp::v1::rules_table::Weapon>,
    ) -> Result<Option<WeaponRow>, RulesError> {
        row.map(|row| WeaponRow::read(field, row)).transpose()
    }
}

/// Combat's rows that are not a weapon's, and the materials' rows, typed at
/// load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CombatRows {
    /// `combat.under_attack_window_ms`, never negative: how long after a
    /// damage an asset's attacker counts for `RETURN_FIRE` (decision 4).
    pub under_attack_window_ms: Ms,
    /// `materials.blast_levels`, each one to three.
    pub blast_levels: BlastLevels,
    /// `materials.bedrock_layers_voxels`, at least one.
    pub bedrock_layers_voxels: i16,
    /// `economy.salvage_percent`: a wreck's worth, as a whole percent of the
    /// thing's build cost.
    pub salvage_percent: u32,
}

impl CombatRows {
    /// Read the rows, refusing what the tick could not use.
    ///
    /// # Errors
    ///
    /// [`RulesError::MissingBlock`] for an absent `combat`, `materials`,
    /// `materials.blast_levels` or `economy` block, and
    /// [`RulesError::OutOfRange`] for a window that is not positive, a salvage
    /// percent past 100, a blast level outside
    /// one to three, zero bedrock layers or a layer count past a coordinate.
    pub fn read(message: &gp::v1::RulesTable) -> Result<CombatRows, RulesError> {
        let combat = message
            .combat
            .as_ref()
            .ok_or(RulesError::MissingBlock("combat"))?;
        let materials = message
            .materials
            .as_ref()
            .ok_or(RulesError::MissingBlock("materials"))?;
        let levels = materials
            .blast_levels
            .as_ref()
            .ok_or(RulesError::MissingBlock("materials.blast_levels"))?;
        let economy = message
            .economy
            .as_ref()
            .ok_or(RulesError::MissingBlock("economy"))?;
        if combat.under_attack_window_ms <= 0 {
            return Err(RulesError::OutOfRange {
                field: "combat.under_attack_window_ms".to_owned(),
                value: format!(
                    "{}; a window of no time remembers no attack, so RETURN_FIRE would never fire",
                    combat.under_attack_window_ms
                ),
            });
        }
        if economy.salvage_percent > 100 {
            return Err(RulesError::OutOfRange {
                field: "economy.salvage_percent".to_owned(),
                value: format!(
                    "{}; a wreck worth more than the thing it was would make every death pay",
                    economy.salvage_percent
                ),
            });
        }
        let blast_levels = BlastLevels::new(levels.dirt, levels.stone, levels.ore, levels.vent)
            .map_err(|error| RulesError::OutOfRange {
                field: format!("materials.blast_levels.{}", error.kind),
                value: format!(
                    "{}; the catalogue holds 1 to {} levels a kind",
                    error.found,
                    BlastLevels::MAX
                ),
            })?;
        if materials.bedrock_layers_voxels == 0 {
            return Err(RulesError::OutOfRange {
                field: "materials.bedrock_layers_voxels".to_owned(),
                value: "0; the support rule needs a floor".to_owned(),
            });
        }
        let bedrock_layers_voxels =
            i16::try_from(materials.bedrock_layers_voxels).map_err(|_| RulesError::OutOfRange {
                field: "materials.bedrock_layers_voxels".to_owned(),
                value: format!(
                    "{}; a layer count is a voxel length",
                    materials.bedrock_layers_voxels
                ),
            })?;
        Ok(CombatRows {
            under_attack_window_ms: Ms::new(combat.under_attack_window_ms),
            blast_levels,
            bedrock_layers_voxels,
            salvage_percent: economy.salvage_percent,
        })
    }
}
