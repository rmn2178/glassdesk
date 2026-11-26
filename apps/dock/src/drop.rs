//! OLE drop target for files dragged onto the dock. The decisions live in
//! `app`; this only adapts the COM interface.

use windows::{
    Win32::{
        Foundation::POINTL,
        System::{
            Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL},
            Ole::{CF_HDROP, DROPEFFECT, IDropTarget, IDropTarget_Impl, ReleaseStgMedium},
            SystemServices::MODIFIERKEYS_FLAGS,
        },
        UI::Shell::{DragQueryFileW, HDROP},
    },
    core::{Ref, Result, implement},
};

use crate::app;

#[implement(IDropTarget)]
pub struct DropTarget;

/// Writes the chosen effect, limited to those the drag source allows.
fn answer(effect: *mut DROPEFFECT, chosen: DROPEFFECT) {
    if effect.is_null() {
        return;
    }
    // SAFETY: OLE passes a valid pointer holding the allowed effects.
    unsafe { *effect = DROPEFFECT((*effect).0 & chosen.0) };
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let files = data.ok().map(dropped_files).unwrap_or_default();
        answer(effect, app::drag_enter(files, point.x, point.y));
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        answer(effect, app::drag_over(point.x, point.y));
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        app::drag_leave();
        Ok(())
    }

    fn Drop(
        &self,
        _data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        answer(effect, app::drop_files(point.x, point.y));
        Ok(())
    }
}

fn dropped_files(data: &IDataObject) -> Vec<String> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // SAFETY: The medium is released after reading; each buffer is sized from
    // the length DragQueryFileW reports.
    unsafe {
        let Ok(mut medium) = data.GetData(&format) else {
            return Vec::new();
        };
        let drop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let files = (0..count)
            .map(|index| {
                let len = DragQueryFileW(drop, index, None) as usize;
                let mut buffer = vec![0u16; len + 1];
                DragQueryFileW(drop, index, Some(&mut buffer));
                String::from_utf16_lossy(&buffer[..len])
            })
            .collect();
        ReleaseStgMedium(&mut medium);
        files
    }
}
