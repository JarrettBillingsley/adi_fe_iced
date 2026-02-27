
use std::collections::{ BTreeMap, VecDeque };
use std::ops::{ Bound };
use std::cell::{ RefCell, Ref, RefMut };

use iced::widget::text::{ Span };
use iced::{ Element, Font, color, Length, Border, Padding };
use iced::font::{ Weight };
use iced::widget::{ pane_grid, text, column, row, span, container, scrollable, text::Rich, button,
	space };

use better_panic::{ Settings as PanicSettings, Verbosity as PanicVerbosity };

use rand::prelude::*;

mod sparse_list;
use sparse_list::{ sparse_list, IContent, Change as ListChange };

fn main() -> iced::Result {
	setup_panic();
	iced::application(AdiFE::default, AdiFE::update, AdiFE::view)
		.font(CONSOLAS_BYTES)
		.run()
}

fn setup_panic() {
	PanicSettings::new()
		.lineno_suffix(true)
		.most_recent_first(false)
		.verbosity(PanicVerbosity::Full)
	.install();
}

// ------------------------------------------------------------------------------------------------
// Font
// ------------------------------------------------------------------------------------------------

const CONSOLAS_BYTES: &[u8] = include_bytes!("../resources/consolab.ttf");
const CONSOLAS_FONT: Font = Font::with_name("Consolas");

trait FontEx {
	fn bold(&self) -> Font;
}

impl FontEx for Font {
	fn bold(&self) -> Font {
		Font {
			weight: Weight::Bold,
			..*self
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Message
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum CodeViewChangeKind {
	Split,
	Delete,
	Modify,
}

#[derive(Debug, Clone, Copy)]
enum Message {
	PaneDragged(pane_grid::DragEvent),
	PaneResized(pane_grid::ResizeEvent),
	OperandClicked { ea: usize, opn: usize },

	CodeViewChange { ea: usize, kind: CodeViewChangeKind },
}

// ------------------------------------------------------------------------------------------------
// CodeLink
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Copy, Clone)]
enum CodeLink {
	Operand { ea: usize, opn: usize },
}

impl CodeLink {
	fn into_message(self) -> Message {
		match self {
			CodeLink::Operand { ea, opn } => {
				Message::OperandClicked { ea, opn }
			}
		}
	}
}

// ------------------------------------------------------------------------------------------------
// TextBB
// ------------------------------------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq)]
struct TextBB {
	ea: usize,
	code: Vec<(String, String)> // mnemonic, operand
}

impl TextBB {
	fn label(&self) -> String {
		format!("PRG0_loc_{:04X}", self.ea)
	}

	fn render(&self) -> Vec<Span<'_, CodeLink>> {
		let mut spans: Vec<iced::widget::text::Span<'_, CodeLink, Font>> =
			Vec::with_capacity(5 * (self.code.len() + 1));
		spans.push(span(self.label()).color(color!(0xA06000)));
		spans.push(span(":\n").color(color!(0xFFFFFF)));

		for (i, (mnemonic, operands)) in self.code.iter().enumerate() {
			spans.push(span("    "));
			spans.push(span(mnemonic).color(color!(0xFF0000)));
			spans.push(span(" "));
			spans.push(
				span(operands)
				.color(color!(0xFFFFFF))
				.link(CodeLink::Operand { ea: self.ea + i, opn: 0 })
			);
			spans.push(span("\n"));
		}

		spans
	}
}

// ------------------------------------------------------------------------------------------------
// Dummy code data
// ------------------------------------------------------------------------------------------------

const NUM_CODE_SPANS: usize = 50;

const MNEMONICS: &[&'static str] = &[
	"lda", "sta", "bpl", "jsr", "rts", "dex", "pha",
];

const OPERANDS: &[&'static str] = &[
	"#30", "[PPUSCROLL]", "[arr + X]", "label", "#$69",
];

fn dummy_code_data() -> &'static [TextBB] {
	use std::sync::LazyLock;
	static RET: LazyLock<Vec<TextBB>> = LazyLock::new(|| {
		let mut bbs = vec![];
		let mut ea = 0;
		let mut rng = rand::rng();

		for _ in 0 .. NUM_CODE_SPANS {
			let len = rng.random_range(1 ..= 10);

			bbs.push(TextBB {
				ea,
				code: (0 .. len).map(|_| (
					(*MNEMONICS.choose(&mut rng).unwrap()).into(),
					(*OPERANDS.choose(&mut rng).unwrap()).into()
				)).collect()
			});

			ea += len;
		}

		bbs
	});

	&*RET
}

// ------------------------------------------------------------------------------------------------
// NamesPane
// ------------------------------------------------------------------------------------------------

struct NamesPane {
	names: Vec<String>,
}

impl NamesPane {
	fn new() -> Self {
		let bbs = dummy_code_data();
		let mut names = Vec::with_capacity(bbs.len());

		for bb in bbs.iter() {
			names.push(bb.label());
		}

		Self { names }
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		let ui = scrollable(column(self.names.iter().map(|name| {
			text(name).font(CONSOLAS_FONT.bold()).into()
		})).width(Length::Fill).padding(10));

		(ui.into(), "Names".into())
	}
}

// ------------------------------------------------------------------------------------------------
// CodePane
// ------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SpanKind {
	Unk,
	Code(usize),
	Data,
	Ana,
	AnaCode,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct AdiSpan {
	seg:   u16,
	start: usize,
	end:   usize,
	kind:  SpanKind,
}

struct DummySegment {
	end: usize,
	bbs: Vec<TextBB>,
	spans: BTreeMap<usize, AdiSpan>,
	changes: RefCell<VecDeque<ListChange>>,
}

impl DummySegment {
	fn new() -> Self {
		let bbs = dummy_code_data().to_vec();
		Self {
			end: 0x10000,
			spans: bbs.iter().enumerate().map(|(i, bb)| {
				(bb.ea,
				AdiSpan {
					seg: 0,
					start: bb.ea,
					end: bb.ea + bb.code.len(),
					kind: SpanKind::Code(i),
				})
			}).collect(),
			bbs,
			changes: RefCell::new(VecDeque::new()),
		}
	}

	fn get_bb(&self, bbidx: usize) -> &TextBB {
		&self.bbs[bbidx]
	}

	fn try_split(&mut self, old_ea: usize) {
		let old_span = self.spans.get(&old_ea).unwrap();
		let SpanKind::Code(bbidx) = old_span.kind else { panic!() };
		let old_bb = &mut self.bbs[bbidx];
		let cur_len = old_bb.code.len();

		if cur_len >= 2 {
			let old_len = cur_len / 2;
			let new_len = cur_len - old_len;
			let new_code = old_bb.code.split_off(old_len);
			assert!(new_code.len() == new_len);

			let new_ea = old_ea + old_len;
			let new_bbid = self.bbs.len();

			self.bbs.push(TextBB {
				ea: new_ea,
				code: new_code
			});

			self.insert(old_ea, AdiSpan {
				seg: 0,
				start: old_ea,
				end: old_ea + old_len,
				kind: old_span.kind,
			});

			self.insert(new_ea, AdiSpan {
				seg: 0,
				start: new_ea,
				end: new_ea + new_len,
				kind: SpanKind::Code(new_bbid),
			});
		}
	}

	fn modify(&mut self, ea: usize) {
		let span = self.spans.get(&ea).unwrap();
		let SpanKind::Code(bbidx) = span.kind else { panic!() };
		let bb = &mut self.bbs[bbidx];
		bb.code.push(("FUCK".into(), "123".into()));
		self.changes.borrow_mut().push_back(ListChange::Changed { idx: ea });
	}
}

impl<'a> IContent<'a, AdiSpan> for DummySegment {
	fn len(&self) -> usize {
		self.spans.len()
	}

	fn domain(&self) -> usize {
		self.end
	}

	fn first(&self) -> Option<usize> {
		self.spans.keys().copied().nth(0)
	}

	fn last(&self) -> Option<usize> {
		self.spans.keys().copied().nth_back(0)
	}

	fn get(&self, idx: usize) -> Option<&AdiSpan> {
		self.spans.get(&idx)
	}

	fn items_before(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		let mut iter = self.spans.range(..= idx);
		iter.next_back();
		Box::new(iter.rev().map(|(idx, span)| (*idx, span)))
	}

	fn items_after(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		Box::new(self.spans.range((Bound::Excluded(idx), Bound::Unbounded))
			.map(|(idx, span)| (*idx, span)))
	}

	fn items_at_and_before(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		let iter = self.spans.range(..= idx);
		Box::new(iter.rev().map(|(idx, span)| (*idx, span)))
	}

	fn items_at_and_after(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		Box::new(self.spans.range((Bound::Included(idx), Bound::Unbounded))
			.map(|(idx, span)| (*idx, span)))
	}

	fn insert(&mut self, idx: usize, val: AdiSpan) -> Option<AdiSpan> {
		let ret = self.spans.insert(idx, val.clone());

		match ret {
			None =>
				self.changes.borrow_mut().push_back(ListChange::Added { idx }),
			Some(ref old) if *old != val =>
				self.changes.borrow_mut().push_back(ListChange::Changed { idx }),
			_ => {}
		}

		ret
	}

	fn remove(&mut self, idx: usize) -> bool {
		let ret = self.spans.remove(&idx).is_some();

		if ret {
			self.changes.borrow_mut().push_back(ListChange::Removed { idx });
		}

		ret
	}

	fn changes(&'a self) -> Ref<'a, VecDeque<ListChange>> {
		self.changes.borrow()
	}

	fn changes_mut(&'a self) -> RefMut<'a, VecDeque<ListChange>> {
		self.changes.borrow_mut()
	}
}

struct CodePane {
	seg: DummySegment,
}

impl CodePane {
	fn new() -> Self {
		Self {
			seg: DummySegment::new(),
		}
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		let ui = container(sparse_list(
			&self.seg,
			|ea, span: &AdiSpan| {
				println!("manifesting bb @ ea {:04X}", ea);

				let SpanKind::Code(bbidx) = span.kind else { panic!() };
				let bb = self.seg.get_bb(bbidx);

				use CodeViewChangeKind::*;

				container(column![
					row![
						button("split" ).on_press(Message::CodeViewChange { ea, kind: Split }),
						iced::widget::space::Space::new().width(10),
						button("delete").on_press(Message::CodeViewChange { ea, kind: Delete }),
						iced::widget::space::Space::new().width(10),
						button("modify").on_press(Message::CodeViewChange { ea, kind: Modify }),
					],
					Rich::with_spans(bb.render())
						.on_link_click(CodeLink::into_message)
						.font(CONSOLAS_FONT.bold()),
					row![
						button("split" ).on_press(Message::CodeViewChange { ea, kind: Split }),
						iced::widget::space::Space::new().width(10),
						button("delete").on_press(Message::CodeViewChange { ea, kind: Delete }),
						iced::widget::space::Space::new().width(10),
						button("modify").on_press(Message::CodeViewChange { ea, kind: Modify }),
					],
				])
				.width(Length::Fill)
				.style(move |_theme| {
					container::Style::default().border(
						Border::default().color(color!(0xFFFFFF)).width(0.3))
				})
				.into()
			})
		)
		.width(Length::Fill)
		.height(Length::Fill)
		.padding(Padding::from([0, 10]))
		.style(move |_theme| {
			container::Style::default().background(color!(0x101010))
		});

		(ui.into(), "Code".into())
	}

	fn change(&mut self, ea: usize, kind: CodeViewChangeKind) {
		match kind {
			CodeViewChangeKind::Split => {
				println!("split bb @ {:04X}", ea);
				self.seg.try_split(ea);
			}
			CodeViewChangeKind::Delete => {
				println!("delete bb @ {:04X}", ea);
				self.seg.remove(ea);
			}
			CodeViewChangeKind::Modify => {
				println!("modify bb @ {:04X}", ea);
				self.seg.modify(ea);
			}
		}
	}
}

// ------------------------------------------------------------------------------------------------
// PaneState
// ------------------------------------------------------------------------------------------------

enum PaneState {
	Names(NamesPane),
	Code(CodePane),
}

impl PaneState {
	fn new_names() -> Self {
		Self::Names(NamesPane::new())
	}

	fn new_code() -> Self {
		Self::Code(CodePane::new())
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		match self {
			PaneState::Names(n) => n.view(),
			PaneState::Code(c)  => c.view(),
		}
	}

	fn as_code_mut(&mut self) -> &mut CodePane {
		match self {
			PaneState::Code(c) => c,
			_ => panic!(),
		}
	}
}

// ------------------------------------------------------------------------------------------------
// AdiFE
// ------------------------------------------------------------------------------------------------

struct AdiFE {
	panes: pane_grid::State<PaneState>,
	#[allow(dead_code)]
	name_pane: pane_grid::Pane,
	code_pane: pane_grid::Pane,
}

impl AdiFE {
	fn new() -> Self {
		let (mut panes, name_pane) = pane_grid::State::new(PaneState::new_names());
		let (code_pane, split) = panes.split(
			pane_grid::Axis::Vertical, name_pane, PaneState::new_code()).unwrap();
		panes.resize(split, 0.2);

		Self { panes, name_pane, code_pane }
	}

	fn update(&mut self, message: Message) {
		match message {
			Message::PaneDragged(de) => {
				println!("dragged {:?}", de);
			}
			Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
				self.panes.resize(split, ratio);
			}
			Message::OperandClicked { ea, opn } => {
				println!("clicked operand {} of instruction at {:04X}", opn, ea);
			}

			Message::CodeViewChange { ea, kind } => {
				self.panes.get_mut(self.code_pane).unwrap().as_code_mut().change(ea, kind);
			}
		}
	}

	fn view(&self) -> Element<'_, Message> {
		column![
			// trying to extract this callback into its own method is an exercise in frustration.
			// just leave it here unless you want to have the Worst Types and Where Clauses Ever.
			pane_grid(&self.panes, |_pane, state, _is_maximized| {
				let (content, title) = state.view();
				let title = text(title).size(20).font(Font::DEFAULT.bold());

				pane_grid::Content::new(content)
					.title_bar(pane_grid::TitleBar::new(title).padding(10))
			})
			.on_drag(Message::PaneDragged)
			.on_resize(10, Message::PaneResized)
			.min_size(200),

			space().height(50)
		].into()
	}
}

impl Default for AdiFE {
	fn default() -> Self {
		Self::new()
	}
}