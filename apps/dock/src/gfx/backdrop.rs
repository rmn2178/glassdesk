//! Wallpaper changes affect the host sample, not layout, icons or input state.

use anyhow::Result;
use windows::{
    UI::Composition::{CompositionBackdropBrush, ContainerVisual, SpriteVisual},
    core::Interface,
};

use super::Gfx;

impl Gfx {
    /// Rebinds only existing host backdrop brushes, including nested popup
    /// surfaces. All backdrop brushes in Nosih are host brushes. Solid themes
    /// allocate no replacement brushes. Called only on a change event.
    pub fn refresh_backdrops(&self, root: &ContainerVisual) -> Result<usize> {
        let mut refreshed = 0;
        if let Ok(sprite) = root.cast::<SpriteVisual>()
            && let Ok(brush) = sprite.Brush()
            && brush.cast::<CompositionBackdropBrush>().is_ok()
        {
            sprite.SetBrush(&self.compositor.CreateHostBackdropBrush()?)?;
            refreshed += 1;
        }
        for child in root.Children()? {
            if let Ok(container) = child.cast::<ContainerVisual>() {
                refreshed += self.refresh_backdrops(&container)?;
            }
        }
        Ok(refreshed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        UI::Color,
        Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        },
    };
    use windows_numerics::Vector3;

    #[test]
    #[ignore = "Native compositor regression; no desktop or wallpaper changes"]
    fn wallpaper_refresh_preserves_content_geometry_and_solid_materials() -> Result<()> {
        // SAFETY: A private dispatcher and compositor on this test thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let c = &gfx.compositor;
        let root = c.CreateContainerVisual()?;
        let popup = c.CreateContainerVisual()?;
        popup.SetOffset(Vector3::new(17.0, 31.0, 0.0))?;
        popup.SetOpacity(0.75)?;
        root.Children()?.InsertAtTop(&popup)?;
        let background = c.CreateSpriteVisual()?;
        let old_brush = c.CreateHostBackdropBrush()?;
        background.SetBrush(&old_brush)?;
        popup.Children()?.InsertAtBottom(&background)?;
        let icon = c.CreateSpriteVisual()?;
        let color = c.CreateColorBrushWithColor(Color {
            A: 255,
            R: 50,
            G: 100,
            B: 150,
        })?;
        icon.SetBrush(&color)?;
        popup.Children()?.InsertAtTop(&icon)?;
        // Empty sprites and shape visuals are valid siblings, not failures.
        popup.Children()?.InsertAtTop(&c.CreateSpriteVisual()?)?;
        popup.Children()?.InsertAtTop(&c.CreateShapeVisual()?)?;

        assert_eq!(gfx.refresh_backdrops(&root)?, 1);
        assert_ne!(background.Brush()?, old_brush.cast()?);
        assert_eq!(icon.Brush()?, color.cast()?);
        assert_eq!(root.Children()?.First()?.Current()?, popup.cast()?);
        assert_eq!(popup.Offset()?, Vector3::new(17.0, 31.0, 0.0));
        assert_eq!(popup.Opacity()?, 0.75);
        popup.Children()?.Remove(&background)?;
        assert_eq!(gfx.refresh_backdrops(&root)?, 0);
        assert_eq!(icon.Brush()?, color.cast()?);
        Ok(())
    }
}
