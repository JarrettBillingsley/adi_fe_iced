
use iced::{ Element, Font, color, Length, Border };
use iced::font::{ Weight };
use iced::widget::{ pane_grid, text, column, span, container, scrollable, text::Rich };

use rand::prelude::*;

fn main() -> iced::Result {
	iced::application(AdiFE::default, AdiFE::update, AdiFE::view)
		.font(CONSOLAS_BYTES)
		.run()
}

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

#[derive(Debug, Copy, Clone)]
enum CodeLink {
	Operand { ea: usize, opn: usize },
}

struct TextBB {
	ea: usize,
	code: Vec<(String, String)> // mnemonic, operand
}

struct AdiFE {
	panes: pane_grid::State<PaneState>,
}

enum PaneState {
	Names { names: Vec<String> },
	Code { bbs: Vec<TextBB> },
}

const MNEMONICS: &[&'static str] = &[
	"lda", "sta", "bpl", "jsr", "rts", "dex", "pha",
];

const OPERANDS: &[&'static str] = &[
	"#30", "[PPUSCROLL]", "[arr + X]", "label", "#$69",
];

impl PaneState {
	fn new_names(code: &PaneState) -> Self {
		match code {
			PaneState::Code { bbs } => {
				let mut names = Vec::with_capacity(bbs.len());

				for TextBB { ea, .. } in bbs.iter() {
					names.push(format!("PRG0_loc_{:04X}", ea));
				}

				PaneState::Names { names }
			}
			_ => unreachable!(),
		}
	}

	fn new_code() -> Self {
		let mut bbs = vec![];
		let mut ea = 0;
		let mut rng = rand::rng();

		for _ in 0 .. 1000 {
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

		PaneState::Code { bbs }
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		match self {
			PaneState::Names { names } => {
				let ui = scrollable(column(names.iter().map(|name| {
					text(name).font(CONSOLAS_FONT.bold()).into()
				})).width(Length::Fill).padding(10));

				(ui.into(), "Names".into())
			}
			PaneState::Code { bbs } => {
				let ui = container(
					scrollable(column(bbs.iter().map(|TextBB { ea, code }| {
						let mut spans: Vec<iced::widget::text::Span<'_, CodeLink, Font>> =
							Vec::with_capacity(5 * (code.len() + 1));
						spans.push(span(format!("PRG0_loc_{:04X}", ea)).color(color!(0xA06000)));
						spans.push(span(":\n").color(color!(0xFFFFFF)));

						for (i, (mnemonic, operands)) in code.iter().enumerate() {
							spans.push(span("    "));
							spans.push(span(mnemonic).color(color!(0xFF0000)));
							spans.push(span(" "));
							spans.push(
								span(operands)
								.color(color!(0xFFFFFF))
								.link(CodeLink::Operand { ea: ea + i, opn: 0 })
							);
							spans.push(span("\n"));
						}

						container(Rich::with_spans(spans)
							.on_link_click(|link| {
								match link {
									CodeLink::Operand { ea, opn } => {
										Message::OperandClicked { ea, opn }
									}
								}
							})
							.font(CONSOLAS_FONT.bold())
						)
						.width(Length::Fill)
						.style(move |_theme| {
							container::Style::default().border(
								Border::default().color(color!(0xFFFFFF)).width(0.3))
						})
						.into()
					})))
				)
				.width(Length::Fill)
				.height(Length::Fill)
				.padding(10)
				.style(move |_theme| {
					container::Style::default().background(color!(0x101010))
				});

				(ui.into(), "Code".into())
			}
		}
	}
}

#[derive(Debug, Clone, Copy)]
enum Message {
	PaneDragged(pane_grid::DragEvent),
	PaneResized(pane_grid::ResizeEvent),
	OperandClicked { ea: usize, opn: usize },
}

impl AdiFE {
	fn new() -> Self {
		let code = PaneState::new_code();
		let (mut panes, orig) = pane_grid::State::new(PaneState::new_names(&code));
		let (_, split) = panes.split(
			pane_grid::Axis::Vertical, orig, code).unwrap();
		panes.resize(split, 0.3);

		Self {
			panes,
		}
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
		}
	}

	fn view(&self) -> Element<'_, Message> {
		// trying to extract this callback into its own method is an exercise in frustration.
		// just leave it here unless you want to have the Worst Types and Where Clauses Ever.
		pane_grid(&self.panes, |_pane, state, _is_maximized| {
			let (content, title) = state.view();
			let title = text(title).size(20).font(Font::DEFAULT.bold());

			pane_grid::Content::new(content)
				.title_bar(
					pane_grid::TitleBar::new(title)
					.padding(10)
				)
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