
use std::collections::{ BTreeMap, VecDeque };
use std::ops::{ Bound };
use std::cell::{ RefCell, Ref, RefMut };

use iced::widget::text::{ Span };
use iced::{ Element, Font, color, Length, Border, Padding };
use iced::font::{ Weight };
use iced::widget::{ pane_grid, text, column, row, span, container, scrollable, text::Rich, button };

use rand::prelude::*;

mod sparse_list;
use sparse_list::{ sparse_list, IContent, NewChange };

fn main() -> iced::Result {
	iced::application(AdiFE::default, AdiFE::update, AdiFE::view)
		.font(CONSOLAS_BYTES)
		.run()
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
enum Message {
	PaneDragged(pane_grid::DragEvent),
	PaneResized(pane_grid::ResizeEvent),
	OperandClicked { ea: usize, opn: usize },

	SplitBB { ea: usize },
	DeleteBB { ea: usize },
	ModifyBB { ea: usize },
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

const NUM_CODE_SPANS: usize = 30;

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
	bbs: BTreeMap<usize, AdiSpan>,
	changes: RefCell<VecDeque<NewChange>>,
}

impl DummySegment {
	fn new() -> Self {
		Self {
			end: 0x10000,
			bbs: dummy_code_data().iter().enumerate().map(|(i, bb)| {
				(bb.ea,
				AdiSpan {
					seg: 0,
					start: bb.ea,
					end: bb.ea + bb.code.len(),
					kind: SpanKind::Code(i),
				})
			}).collect(),
			changes: RefCell::new(VecDeque::new()),
		}
	}
}

impl<'a> IContent<'a, AdiSpan> for DummySegment {
	fn len(&self) -> usize {
		self.bbs.len()
	}

	fn domain(&self) -> usize {
		self.end
	}

	fn first(&self) -> Option<usize> {
		self.bbs.keys().copied().nth(0)
	}

	fn last(&self) -> Option<usize> {
		self.bbs.keys().copied().nth_back(0)
	}

	fn get(&self, idx: usize) -> Option<&AdiSpan> {
		self.bbs.get(&idx)
	}

	fn items_before(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		let mut iter = self.bbs.range(..= idx);
		iter.next_back();
		Box::new(iter.map(|(idx, span)| (*idx, span)))
	}

	fn items_after(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		Box::new(self.bbs.range((Bound::Excluded(idx), Bound::Unbounded))
			.map(|(idx, span)| (*idx, span)))
	}

	fn items_at_and_before(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		let iter = self.bbs.range(..= idx);
		Box::new(iter.map(|(idx, span)| (*idx, span)))
	}

	fn items_at_and_after(&'a self, idx: usize)
	-> Box<dyn DoubleEndedIterator<Item = (usize, &'a AdiSpan)> + 'a> {
		Box::new(self.bbs.range((Bound::Included(idx), Bound::Unbounded))
			.map(|(idx, span)| (*idx, span)))
	}

	fn insert(&mut self, idx: usize, val: AdiSpan) -> Option<AdiSpan> {
		let ret = self.bbs.insert(idx, val.clone());

		match ret {
			None =>
				self.changes.borrow_mut().push_back(NewChange::Added { idx }),
			Some(ref old) if *old != val =>
				self.changes.borrow_mut().push_back(NewChange::Changed { idx }),
			_ => {}
		}

		ret
	}

	fn remove(&mut self, idx: usize) -> bool {
		let ret = self.bbs.remove(&idx).is_some();

		if ret {
			self.changes.borrow_mut().push_back(NewChange::Removed { idx });
		}

		ret
	}

	fn changes(&'a self) -> Ref<'a, VecDeque<NewChange>> {
		self.changes.borrow()
	}

	fn changes_mut(&'a self) -> RefMut<'a, VecDeque<NewChange>> {
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
			// content: sparse_list::Content::with_items(0x10000, dummy_code_data().to_vec())
		}
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		let ui = container(scrollable(sparse_list(
			&self.seg,
			|ea, span: &AdiSpan| {
				println!("manifesting bb @ ea {:04X}", ea);

				let SpanKind::Code(bbidx) = span.kind else { panic!() };

				let bb = &dummy_code_data()[bbidx];

				container(column![
					row![
						button("split" ).on_press(Message::SplitBB { ea }),
						iced::widget::space::Space::new().width(10),
						button("delete").on_press(Message::DeleteBB { ea }),
						iced::widget::space::Space::new().width(10),
						button("modify").on_press(Message::ModifyBB { ea }),
					],
					Rich::with_spans(bb.render())
						.on_link_click(CodeLink::into_message)
						.font(CONSOLAS_FONT.bold())
				])
				.width(Length::Fill)
				.style(move |_theme| {
					container::Style::default().border(
						Border::default().color(color!(0xFFFFFF)).width(0.3))
				})
				.into()
			})
		))
		.width(Length::Fill)
		.height(Length::Fill)
		.padding(Padding::from([0, 10]))
		.style(move |_theme| {
			container::Style::default().background(color!(0x101010))
		});

		(ui.into(), "Code".into())
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
}

// ------------------------------------------------------------------------------------------------
// AdiFE
// ------------------------------------------------------------------------------------------------

struct AdiFE {
	panes: pane_grid::State<PaneState>,
}

impl AdiFE {
	fn new() -> Self {
		let (mut panes, orig) = pane_grid::State::new(PaneState::new_names());
		let (_, split) = panes.split(
			pane_grid::Axis::Vertical, orig, PaneState::new_code()).unwrap();
		panes.resize(split, 0.2);

		Self { panes }
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

			Message::SplitBB  { ea } => println!("split bb @ {:04X}", ea),
			Message::DeleteBB { ea } => println!("delete bb @ {:04X}", ea),
			Message::ModifyBB { ea } => println!("modify bb @ {:04X}", ea),
		}
	}

	fn view(&self) -> Element<'_, Message> {
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
		.min_size(200)
		.into()
	}
}

impl Default for AdiFE {
	fn default() -> Self {
		Self::new()
	}
}