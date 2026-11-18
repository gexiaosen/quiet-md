//! 模板: built-in starting documents for 新建 › 从模板新建. A template is a
//! DocxXML file under `.quiet/templates/`, read like any other document and
//! copied into a new page — editing that page never touches the template.

use crate::app::Workspace;
use crate::settings::tr;
use crate::theme::Theme;
use crate::ui::{icon, popover};
use gpui::*;

include!(concat!(env!("OUT_DIR"), "/templates.rs"));

/// The built-ins as shipped: (file name, XML). File-name order is menu order.
fn builtin() -> impl Iterator<Item = (&'static str, &'static str)> {
    FILES.iter().filter_map(|(path, bytes)| Some((path.strip_prefix("src/")?, std::str::from_utf8(bytes).ok()?)))
}

impl Workspace {
    /// Write the built-ins that are missing into `.quiet/templates/`. A file
    /// that exists is the user's own copy (edited or deleted on purpose) and is
    /// never overwritten; deleting one brings it back on the next launch.
    pub(crate) fn seed_templates(&mut self) {
        for (name, xml) in builtin() {
            if self.store.with(|s| s.templates_dir().join(name).exists()) { continue; }
            if let Err(e) = self.store.with(|s| s.save_template(name, xml)) { eprintln!("无法写入模板 {name}：{e}"); }
        }
    }

    /// (file name, title) of every template: built-ins first, then the rest.
    pub(crate) fn templates(&self) -> Vec<(String, String)> {
        self.store.with(|s| s.templates()).into_iter().map(|t| (t.name, t.title)).collect()
    }

    /// 从模板新建: the template becomes a page titled 未命名文档 under `parent`.
    pub(crate) fn create_from_template(&mut self, name: &str, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.create_menu = None;
        self.template_menu = None;
        let (n, p) = (name.to_string(), parent.clone());
        if !self.save_before(cx, move |w, window, cx| w.create_from_template(&n, p.clone(), window, cx)) { return; }
        let file_before = self.store.track_file_changes();
        let made = self.store.with(|s| s.template_xml(name))
            .and_then(|xml| crate::docxml::parse(&xml))
            .map(|mut doc| {
                doc.title = crate::model::RichText::plain(crate::model::UNTITLED);
                doc
            })
            .and_then(|doc| self.store.with(|s| s.create_from_in(&doc, parent.as_deref())));
        match made {
            Ok(id) => {
                self.search_open = false;
                self.doc_info = None;
                self.home.sort_menu = false;
                self.entries = self.store.list();
                self.sort();
                match &parent { Some(p) => { self.expanded.insert(p.clone()); } None => self.folded[1] = false }
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
                if let Some(ed) = &self.editor { let f = ed.read(cx).focus.clone(); window.focus(&f, cx); }
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("新建失败：{}", "Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        self.remember_files(file_before, tr("从模板新建", "New from template"));
        cx.notify();
    }

    /// 另存为模板: the open document becomes a starting point for 从模板新建.
    pub(crate) fn save_as_template(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.doc_of(id, cx) else { return };
        let title = crate::settings::display_title(&doc.title.text);
        let stem: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { ' ' } else { c }).collect();
        let stem = stem.trim();
        let stem = if stem.is_empty() { crate::settings::text("未命名模板") } else { stem };
        // `90-` keeps user templates after the built-ins (`01-` … `08-`).
        let mut name = format!("90-{stem}.xml");
        let mut n = 2;
        while self.store.with(|s| s.templates_dir().join(&name).exists()) {
            name = format!("90-{stem}-{n}.xml");
            n += 1;
        }
        match self.store.with(|s| s.save_template(&name, &crate::docxml::serialize(&doc))) {
            Ok(()) => self.show_toast(&crate::localized_format!("已保存为模板「{}」", "Saved as template “{}”", title), cx),
            Err(e) => self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx),
        }
    }

    /// The 从模板新建 list, hanging to the right of the 新建 menu it came from.
    pub(crate) fn render_template_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (parent, pos) = self.template_menu.clone()?;
        let list = self.templates();
        let mut menu = popover()
            .id("template-items")
            .occlude()
            .min_w(px(200.))
            .py(px(crate::ui::MENU_PAD_Y))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.template_menu = None; cx.notify(); }));
        if list.is_empty() {
            menu = menu.child(
                div().px(px(12.)).py(px(8.)).text_size(px(13.)).text_color(Theme::text_caption()).child(tr("还没有模板", "No templates yet")),
            );
        }
        for (i, (name, title)) in list.into_iter().enumerate() {
            let g = SharedString::from(format!("template-{i}"));
            let n = name.clone();
            let p = parent.clone();
            menu = menu.child(
                crate::ui::menu_row(g.clone(), g.clone())
                    .child(icon("doc", 16., Theme::icon()))
                    .child(crate::ui::menu_label(SharedString::from(title), g.clone(), gpui::white()))
                    .on_click(cx.listener(move |w, _, window, cx| w.create_from_template(&n, p.clone(), window, cx))),
            );
        }
        Some(deferred(anchored().anchor(Anchor::TopLeft).position(pos).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(3).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    // Not `use super::*`: it would bring in gpui's `test` attribute macro.
    use super::builtin;

    /// Every shipped template is a document the parser accepts, with a title
    /// and something in it, and survives a write/read round trip unchanged.
    #[test]
    fn every_builtin_is_a_usable_document() {
        let mut seen = vec![];
        for (name, xml) in builtin() {
            let doc = crate::docxml::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
            let title = crate::model::display_title(&doc.title.text);
            assert!(!title.trim().is_empty(), "{name} has no title");
            assert!(!doc.blocks.is_empty(), "{name} is empty");
            let again = crate::docxml::parse(&crate::docxml::serialize(&doc)).unwrap_or_else(|e| panic!("{name} round trip: {e}"));
            assert_eq!(again.blocks.len(), doc.blocks.len(), "{name} loses blocks on a round trip");
            seen.push(name);
        }
        assert_eq!(seen.len(), 8, "{seen:?}");
        assert!(seen.first().is_some_and(|n| n.starts_with("01-")), "built-ins are named in menu order");
    }
}
