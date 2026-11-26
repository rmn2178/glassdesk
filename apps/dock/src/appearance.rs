//! The theme Settings chose for the dock, the menu bar and their menus and
//! panels. `Auto`, frosted and liquid glass follow Windows' light or dark mode;
//! explicit light/dark and the clear palette keep their chosen tone.
//! The menu bar takes the dock's theme unless it was given its own (#54).
//!
//! It is process state, like the Windows theme it replaces: everything that
//! draws reads `current()` when it builds, and a change rebuilds through the
//! same keys a Windows theme change does.

use std::sync::atomic::{AtomicU8, Ordering};

use nosih_config::Appearance;
use nosih_shell::system::{transparency_enabled, uses_light_theme};

/// What the glass is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    /// The dock as it always was: the light or dark macOS glass.
    Standard,
    Frosted,
    Clear,
    LiquidGlass,
}

#[cfg(test)]
impl Material {
    pub const ALL: [Self; 4] = [
        Self::Standard,
        Self::Frosted,
        Self::Clear,
        Self::LiquidGlass,
    ];
}

/// What a surface is drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub material: Material,
    /// Light surfaces with dark content; otherwise the reverse.
    pub light: bool,
    /// See `system::transparency_enabled`: without it every material is solid.
    pub glass: bool,
}

static CHOICE: AtomicU8 = AtomicU8::new(Appearance::LiquidGlass as u8);
/// The menu bar's own theme, or `SAME` for the dock's.
static MENU_BAR_CHOICE: AtomicU8 = AtomicU8::new(SAME);
/// Stored for "the menu bar takes the dock's theme": no theme has this value.
const SAME: u8 = u8::MAX;

pub fn set(choice: Appearance) {
    CHOICE.store(choice as u8, Ordering::Relaxed);
}

/// The menu bar's own theme; `None` gives it the dock's.
pub fn set_menu_bar(choice: Option<Appearance>) {
    MENU_BAR_CHOICE.store(encode_menu_bar(choice), Ordering::Relaxed);
}

fn encode_menu_bar(choice: Option<Appearance>) -> u8 {
    choice.map_or(SAME, |choice| choice as u8)
}

fn decode_menu_bar(stored: u8) -> Option<Appearance> {
    (stored != SAME).then(|| decode(stored))
}

const CHOICES: [Appearance; 6] = [
    Appearance::Light,
    Appearance::Dark,
    Appearance::Frosted,
    Appearance::Clear,
    Appearance::LiquidGlass,
    Appearance::Auto,
];

fn decode(stored: u8) -> Appearance {
    CHOICES
        .into_iter()
        .find(|choice| *choice as u8 == stored)
        .unwrap_or_default()
}

#[must_use]
pub fn resolve(choice: Appearance, system_light: bool, glass: bool) -> Look {
    let (material, light) = match choice {
        Appearance::Auto => (Material::Standard, system_light),
        Appearance::Light => (Material::Standard, true),
        Appearance::Dark => (Material::Standard, false),
        Appearance::Frosted => (Material::Frosted, system_light),
        Appearance::Clear => (Material::Clear, false),
        Appearance::LiquidGlass => (Material::LiquidGlass, system_light),
    };
    Look {
        material,
        light,
        glass,
    }
}

fn look_of(choice: Appearance) -> Look {
    let system_light = matches!(
        choice,
        Appearance::Auto | Appearance::Frosted | Appearance::LiquidGlass
    ) && uses_light_theme();
    resolve(choice, system_light, transparency_enabled())
}

/// The dock's look, and its menus' and panels'.
#[must_use]
pub fn current() -> Look {
    look_of(decode(CHOICE.load(Ordering::Relaxed)))
}

/// The menu bar's look, and its menus' and panels': its own theme, or the
/// dock's.
#[must_use]
pub fn menu_bar() -> Look {
    let dock = decode(CHOICE.load(Ordering::Relaxed));
    look_of(decode_menu_bar(MENU_BAR_CHOICE.load(Ordering::Relaxed)).unwrap_or(dock))
}

/// Whether the dock's menus and labels are drawn light, with dark content.
#[must_use]
pub fn light() -> bool {
    current().light
}

/// Whether menus opened from the menu bar are drawn light.
#[must_use]
pub fn menu_bar_light() -> bool {
    menu_bar().light
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_palettes_remain_independent_of_windows() {
        for system_light in [true, false] {
            assert_eq!(
                resolve(Appearance::Auto, system_light, true).light,
                system_light
            );
            assert!(resolve(Appearance::Light, system_light, true).light);
            for dark in [Appearance::Dark, Appearance::Clear] {
                assert!(!resolve(dark, system_light, true).light);
            }
        }
    }

    #[test]
    fn frosted_follows_windows_palette_and_preserves_the_transparency_policy() {
        for light in [false, true] {
            for glass in [false, true] {
                let look = resolve(Appearance::Frosted, light, glass);
                assert_eq!(look.material, Material::Frosted);
                assert_eq!(look.light, light);
                assert_eq!(look.glass, glass);
            }
        }
    }

    #[test]
    fn liquid_glass_follows_windows_and_its_transparency_policy() {
        for light in [false, true] {
            for glass in [false, true] {
                assert_eq!(
                    resolve(Appearance::LiquidGlass, light, glass),
                    Look {
                        material: Material::LiquidGlass,
                        light,
                        glass,
                    }
                );
            }
        }
    }

    #[test]
    fn every_choice_decodes_as_itself() {
        for choice in CHOICES {
            assert_eq!(decode(choice as u8), choice);
        }
    }

    #[test]
    fn the_menu_bar_keeps_its_own_choice_or_none_for_the_docks() {
        assert_eq!(decode_menu_bar(encode_menu_bar(None)), None);
        for choice in CHOICES {
            assert_ne!(choice as u8, SAME);
            assert_eq!(decode_menu_bar(encode_menu_bar(Some(choice))), Some(choice));
        }
    }
}
