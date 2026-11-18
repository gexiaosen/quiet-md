//! 模板: built-in starting points for a new page. A template is a DocxXML
//! file under `.quiet/templates/`, so it can be edited like any document and is
//! copied into the page — never linked to it.
//!
//! The picker is a sheet over the page, not a menu: 新建 makes a blank document
//! and the page offers the templates as small pages to choose from. Picking one
//! fills that same document in and renames it; `…` › 选择模板 brings the sheet
//! back for a page that already has content.

use crate::app::Workspace;
use crate::model::{Block, BlockKind, Document};
use crate::settings::tr;
use crate::theme::Theme;
use crate::ui::icon;
use gpui::*;

include!(concat!(env!("OUT_DIR"), "/templates.rs"));

/// One card of 选择模板: the template, and enough of its opening lines to look
/// like the page it makes.
pub struct TemplateCard {
    pub name: String,
    pub title: String,
    pub lines: Vec<String>,
}

/// The built-ins as shipped: (file name, XML). File-name order is menu order.
fn builtin() -> impl Iterator<Item = (&'static str, &'static str)> {
    FILES.iter().filter_map(|(path, bytes)| Some((path.strip_prefix("src/")?, std::str::from_utf8(bytes).ok()?)))
}

/// The first few lines of a template's text, as a card shows them.
fn preview_lines(doc: &Document) -> Vec<String> {
    fn first_text(blocks: &[Block]) -> String {
        blocks.iter().map(|b| b.text.text.trim()).find(|t| !t.is_empty()).unwrap_or_default().to_string()
    }
    let mut out = vec![];
    for b in &doc.blocks {
        if out.len() >= 4 {
            break;
        }
        let text = match &b.kind {
            // A template's first table is its skeleton: read its header row.
            BlockKind::Table(t) => t
                .rows
                .first()
                .map(|r| r.iter().map(|c| first_text(&c.blocks)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))
                .unwrap_or_default(),
            _ => b.text.text.clone(),
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        out.push(text.chars().take(30).collect::<String>());
    }
    out
}

impl Workspace {
    /// Write the built-ins that are missing into `.quiet/templates/`. A file
    /// that exists is the user's own copy (edited or deleted on purpose) and is
    /// never overwritten; deleting one brings it back on the next launch.
    pub(crate) fn seed_templates(&mut self) {
        for (name, xml) in builtin() {
            if self.store.with(|s| s.templates_dir().join(name).exists()) {
                continue;
            }
            if let Err(e) = self.store.with(|s| s.save_template(name, xml)) {
                eprintln!("无法写入模板 {name}：{e}");
            }
        }
        self.reload_template_cards();
    }

    /// Read every template's title and opening lines, for the picker's cards.
    pub(crate) fn reload_template_cards(&mut self) {
        self.template_cards = self
            .store
            .with(|s| s.templates())
            .into_iter()
            .map(|t| {
                let lines = self
                    .store
                    .with(|s| s.template_xml(&t.name))
                    .ok()
                    .and_then(|xml| crate::docxml::parse(&xml).ok())
                    .map(|doc| preview_lines(&doc))
                    .unwrap_or_default();
                TemplateCard { name: t.name, title: t.title, lines }
            })
            .collect();
    }

    /// 选择模板 for this page: the sheet opens over it.
    pub(crate) fn pick_template(&mut self, id: &str, cx: &mut Context<Self>) {
        self.template_picker = Some(id.to_string());
        cx.notify();
    }

    pub(crate) fn close_template_picker(&mut self, cx: &mut Context<Self>) {
        self.template_picker = None;
        cx.notify();
    }

    /// Fill the page `id` with a template: its content, and its name.
    ///
    /// The document keeps its id, so its sidebar row and any link to it stay
    /// put — only its content and its title change.
    pub(crate) fn apply_template(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.template_picker.clone() else { return };
        let Some(xml) = self.store.with(|s| s.template_xml(name)).ok() else {
            return self.show_toast_kind(crate::ui::ToastKind::Error, tr("模板读不出来", "Could not read the template"), cx);
        };
        let doc = match crate::docxml::parse(&xml) {
            Ok(doc) => doc,
            Err(e) => return self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx),
        };
        let file_before = self.store.track_file_changes();
        // The page on disk may be cached: drop it so `open` reads what we wrote.
        self.store.with(|s| s.forget_loaded(&id));
        match self.store.with(|s| s.save(&id, &doc)) {
            Ok(_) => {
                self.template_picker = None;
                self.entries = self.store.list();
                self.sort();
                self.open(&id, window, cx);
                if let Some(ed) = &self.editor {
                    let f = ed.read(cx).focus.clone();
                    window.focus(&f, cx);
                }
                self.remember_files(file_before, tr("套用模板", "Template applied"));
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("套用模板失败：{}", "Could not apply the template: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        cx.notify();
    }

    /// 另存为模板: the open document becomes a starting point for 选择模板.
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
            Ok(()) => {
                self.reload_template_cards();
                self.show_toast(&crate::localized_format!("已保存为模板「{}」", "Saved as template “{}”", title), cx)
            }
            Err(e) => self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx),
        }
    }

    /// 选择模板 over the page: a blank document offers it right after 新建, and
    /// `…` › 选择模板 opens it again for a page that already has content.
    pub(crate) fn render_template_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.template_picker.as_ref()?;
        if self.view != crate::app::View::Doc || self.template_cards.is_empty() {
            return None;
        }
        let cards = self.template_cards.iter().enumerate().map(|(i, card)| {
            let (name, title) = (card.name.clone(), card.title.clone());
            let mut page = div().flex().flex_col().gap(px(3.)).px(px(11.)).pt(px(10.)).pb(px(8.)).h(px(96.)).overflow_hidden();
            page = page.child(div().text_size(px(9.5)).line_height(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(title.clone()));
            for line in card.lines.iter() {
                page = page.child(div().text_size(px(8.)).line_height(px(11.)).text_color(Theme::text_caption()).whitespace_nowrap().child(line.clone()));
            }
            div()
                .id(SharedString::from(format!("tpl-card-{i}")))
                .w(px(158.))
                .flex()
                .flex_col()
                .rounded(px(9.))
                .border_1()
                .border_color(Theme::line_border())
                .bg(Theme::bg_body())
                .overflow_hidden()
                .cursor_pointer()
                .hover(|s| s.border_color(Theme::primary_500()))
                // The page the template makes, shrunk: the same white plane, a
                // hairline under it, then the name to click.
                .child(page)
                .child(div().h(px(1.)).bg(Theme::line_border()))
                .child(div().px(px(11.)).py(px(9.)).text_size(px(13.)).text_color(Theme::text()).child(title))
                .on_click(cx.listener(move |w, _, window, cx| w.apply_template(&name, window, cx)))
        });

        let panel = div()
            .w(px(716.))
            .max_h(px(560.))
            .p(px(24.))
            .rounded(px(14.))
            .bg(Theme::popover_bg())
            .border_1()
            .border_color(Theme::line_border())
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(tr("选择模板", "Choose a template")))
                            .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(tr("套用后可以直接改，随时能在 ⋯ 里更换", "Edit it right away; swap templates again from ⋯"))),
                    )
                    .child(
                        div()
                            .id("tpl-close")
                            .size(px(24.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(Theme::fill_hover()))
                            .child(icon("x", 16., Theme::icon()))
                            .on_click(cx.listener(move |w, _, _, cx| w.close_template_picker(cx))),
                    ),
            )
            .child(div().h(px(18.)))
            .child(div().flex().flex_wrap().gap(px(12.)).children(cards))
            .child(div().flex().justify_end().pt(px(14.)).child(
                div()
                    .id("tpl-blank")
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .text_color(Theme::text_caption())
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::fill_hover()).text_color(Theme::text()))
                    .child(tr("不用模板，直接写", "Start blank"))
                    .on_click(cx.listener(move |w, _, _, cx| w.close_template_picker(cx))),
            ));

        // Covers the page rather than floating over the window: it is about
        // this document, and what is beside it is still this document.
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(Theme::bg_body().opacity(0.94))
                .child(panel)
                .into_any_element(),
        )
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
