
use std::fmt::{ Write as FmtWrite };

use adi::{ EA, Program, Span, SpanKind, ImageSliceable,
	BasicBlock, DataItem, IPrintOutput, PrintStyle, FmtResult };

use crate::ui::{ TextEA, CodeViewItem, BasicBlockData, CodeLineData,
	CodeOpData, UnknownData, UnknownLineData, FuncData, FuncDataKind };

// ------------------------------------------------------------------------------------------------
// Rendering stuff
// ------------------------------------------------------------------------------------------------

pub(super) fn render_span(prog: &Program, ea: EA) -> CodeViewItem {
	let span = prog.span_at_ea(ea);

	match span.kind() {
		SpanKind::Unk      => render_unk(prog, &span),
		SpanKind::Code(id) => render_bb(prog, prog.get_bb(id)),
		SpanKind::Data(id) => render_data(prog, prog.get_data(id)),
		_ => panic!("uhhhhh why are we trying to render an in-progress span?"),
	}
}

fn bb_func_differs_from_previous(prog: &Program, bb: &BasicBlock) -> bool {
	let seg = prog.segment_from_ea(bb.ea());
	if let Some(span) = seg.span_before_ea(bb.ea())
		&& let Some(func) = prog.func_that_contains(span.start())
		&& func.id() != bb.func() {
		return true;
	}

	false
}

// if this bb's function differs from the function (if any) that owns the previous span, we need
// to show either a function header or a function piece header.
fn render_bb_header(prog: &Program, bb: &BasicBlock) -> Option<FuncData> {
	let func = prog.get_func(bb.func());

	bb_func_differs_from_previous(prog, bb).then(|| {
		FuncData {
			name: prog.name_of_ea(func.ea()),
			kind: if bb.id() == func.head_id() {
				FuncDataKind::Head {
					attrs: (!func.attrs().is_empty()).then(|| format!("{:?}", func.attrs())),
					entrypoints: func.is_multi_entry().then(||
					func.entrypoints().iter()
						.map(|bbid| prog.name_of_ea(prog.get_bb(*bbid).ea()))
						.collect::<Vec<_>>()
						.join(", "))
				}
			} else {
				FuncDataKind::Piece
			}
		}
	})
}

fn render_bb_code(prog: &Program, bb: &BasicBlock) -> Vec<CodeLineData> {
	let mut ret = vec![];

	let seg = prog.segment_from_ea(bb.ea());
	let seg_name = seg.name();
	let state = bb.mmu_state();

	for inst in bb.insts() {
		let mut bytes = String::new();
		let b = inst.bytes();

		match b.len() {
			1 => write!(bytes, "{:02X}",               b[0]).unwrap(),
			2 => write!(bytes, "{:02X} {:02X}",        b[0], b[1]).unwrap(),
			3 => write!(bytes, "{:02X} {:02X} {:02X}", b[0], b[1], b[2]).unwrap(),
			_ => unreachable!()
		}

		let mut output = UIRenderOutput::new();
		prog.inst_print(inst, state, &mut output).unwrap();
		let (mnemonic, operands) = output.finish();

		ret.push(CodeLineData {
			ea:    TextEA::new(seg_name, prog.fmt_addr(inst.va().0)),
			bytes,
			mnemonic,
			operands,
		});
	}

	ret
}

fn render_bb(prog: &Program, bb: &BasicBlock) -> CodeViewItem {
	let func_header = render_bb_header(prog, bb);
	let label = if prog.get_inrefs(bb.ea()).is_some() {
		prog.name_of_ea(bb.ea())
	} else {
		"".to_string()
	};

	CodeViewItem::BasicBlock(BasicBlockData {
		ea:    bb.ea(),
		func:  func_header,
		label,
		lines: render_bb_code(prog, bb),
	})
}

fn render_data(prog: &Program, data: &DataItem) -> CodeViewItem {
	let ea       = data.ea();
	let seg      = prog.segment_from_ea(ea);
	let state    = prog.mmu_state_at(ea).unwrap_or_else(|| prog.initial_mmu_state());
	let va       = prog.va_from_ea(state, ea);
	let seg_name = seg.name();

	CodeViewItem::DataItem(TextEA::new(seg_name, prog.fmt_addr(va.0)))
}

fn render_unk(prog: &Program, span: &Span) -> CodeViewItem {
	// TODO: these should be configurable
	const UNK_SIZE_CUTOFF: usize = 128;
	const UNK_STRIDE: usize = 16;

	let ea       = span.start();
	let seg      = prog.segment_from_ea(ea);
	let state    = prog.mmu_state_at(ea).unwrap_or_else(|| prog.initial_mmu_state());
	let va       = prog.va_from_ea(state, ea);
	let seg_name = seg.name();

	let mut lines = vec![UnknownLineData {
		ea:    TextEA::new(seg_name, prog.fmt_addr(va.0)),
		bytes: format!("[{} unexplored byte(s)]", span.len())
	}];

	if seg.is_real() {
		let len = span.len().min(UNK_SIZE_CUTOFF);
		let slice = seg.image_slice(ea .. ea + len);
		let data = slice.data();
		let mut addr = prog.fmt_addr(va.0);

		for (i, chunk) in data.chunks(UNK_STRIDE).enumerate() {
			let mut bytes = String::with_capacity(chunk.len() * 3);

			bytes.push_str(&format!("{:02X}", chunk[0]));

			for byte in &chunk[1 ..] {
				bytes.push_str(&format!(" {:02X}", byte));
			}

			addr = prog.fmt_addr(va.0 + i * UNK_STRIDE);
			lines.push(UnknownLineData {
				ea: TextEA::new(seg_name, &addr),
				bytes,
			});
		}

		if span.len() > UNK_SIZE_CUTOFF {
			lines.push(UnknownLineData {
				ea: TextEA::new(seg_name, &addr),
				bytes: "...".into(),
			});
		}
	}

	CodeViewItem::Unknown(UnknownData { lines })
}

// ------------------------------------------------------------------------------------------------
// UIRenderOutput
// ------------------------------------------------------------------------------------------------

struct UIRenderOutput {
	mnemonic:   Option<String>,
	operands:   Vec<CodeOpData>,
	tmp_str:    String,
	tmp_style:  Option<PrintStyle>,
	tmp_opn:    Option<u8>,
}

impl UIRenderOutput {
	fn new() -> Self {
		Self {
			mnemonic:   None,
			operands:   vec![],
			tmp_str:    String::new(),
			tmp_style:  None,
			tmp_opn:    None,
		}
	}

	fn finish(mut self) -> (String, Vec<CodeOpData>) {
		// if there's anything still hanging around in the buffer, output it as plain text
		if !self.tmp_str.is_empty() {
			self.operands.push(
				CodeOpData::new_plain(std::mem::take(&mut self.tmp_str)));
		}

		(self.mnemonic.unwrap_or("???".to_string()), self.operands)
	}
}

impl FmtWrite for UIRenderOutput {
	fn write_str(&mut self, s: &str) -> FmtResult {
		self.tmp_str.write_str(s)
	}
}

impl IPrintOutput for UIRenderOutput {
	fn begin(&mut self, style: PrintStyle) -> FmtResult {
		// if something is in the buffer, it was printed *outside* of any begin/end calls; so output
		// it as plain text.
		if !self.tmp_str.is_empty() {
			self.operands.push(CodeOpData::new_plain(std::mem::take(&mut self.tmp_str)));
		}

		use PrintStyle::*;
		match style {
			Mnemonic => {
				assert!(self.mnemonic.is_none());
			}

			Register | Number | Symbol | String | Comment | Refname | Label => {
				self.tmp_style = Some(style);
			}

			Operand(opn) => {
				self.tmp_opn = Some(opn as u8);
			}

			_ => todo!("a new PrintStyle was added!"),
		}

		Ok(())
	}

	fn end(&mut self, style: PrintStyle) -> FmtResult {
		use PrintStyle::*;
		match style {
			Mnemonic => {
				self.mnemonic = Some(std::mem::take(&mut self.tmp_str));
			}

			Register | Number | Symbol | String | Comment | Refname | Label => {
				self.operands.push(CodeOpData::new(std::mem::take(&mut self.tmp_str),
					self.tmp_style,
					self.tmp_opn)); // works regardless of if we're in an operand
			}

			Operand(opn) => {
				self.tmp_opn = None;

				if !self.tmp_str.is_empty() {
					self.operands.push(CodeOpData::new(std::mem::take(&mut self.tmp_str),
						self.tmp_style,
						Some(opn as u8)));
				}
			}

			_ => todo!("a new PrintStyle was added!"),
		}
		Ok(())
	}
}