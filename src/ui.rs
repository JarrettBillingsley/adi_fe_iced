#![allow(unused)]

use adi::{ EA, SegId, PrintStyle };

// ------------------------------------------------------------------------------------------------
// ------------------------------------------------------------------------------------------------

/// Textual representation of an EA; the segment is a string of the segment name,
/// and the offset is a string of the address in hex.
#[derive(Debug, Clone)]
pub struct TextEA {
	pub seg:  String,
	pub offs: String,
}

impl TextEA {
	pub fn new(seg: &str, offs: &str) -> Self {
		TextEA {
			seg: seg.into(),
			offs: offs.into(),
		}
	}
}

/// Data for one item in the name list.
#[derive(Debug, Clone)]
pub struct NameListData {
	pub name: String,
	pub ea:   EA,
}

/// Data for one segment.
#[derive(Debug, Clone)]
pub struct SegmentData {
	pub segid:    SegId,
	pub name:     String,
	pub is_image: bool,
}

/// Data for a piece of colored text, with an optional operand number.
#[derive(Debug, Clone)]
pub struct CodeText {
	pub text:  String,
	pub style: Option<PrintStyle>,
	pub opn:   Option<u8>,
}

impl CodeText {
	pub fn new(text: &str, style: PrintStyle) -> Self {
		Self::new_raw(text, Some(style), None)
	}

	pub fn new_op(text: &str, style: PrintStyle, opn: u8) -> Self {
		Self::new_raw(text, Some(style), Some(opn))
	}

	pub fn new_unstyled(text: &str) -> Self {
		Self::new_raw(text, None, None)
	}

	pub fn new_raw(text: &str, style: Option<PrintStyle>, opn: Option<u8>) -> Self {
		Self {
			text:  text.into(),
			style,
			opn,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.text.is_empty()
	}
}

/// Data for a single line of rendered code inside a basic block.
#[derive(Debug, Clone)]
pub struct CodeLineData {
	pub ea:       TextEA,
	pub bytes:    String,
	pub mnemonic: String,
	pub operands: Vec<CodeText>,
}

/// Data about a function, to be put at the top of a function in the code listing.
#[derive(Debug, Default, Clone)]
pub struct FunctionData {
	pub name:        String, // if "", don't output a function header.
	pub is_piece:    bool,   // if true, output a function piece header including only the name.
	pub attrs:       String, // if "", no attrs.
	pub entrypoints: String, // if "", single_entry.
}

/// Data for a single basic block of code.
#[derive(Debug, Clone)]
pub struct BasicBlockData {
	pub ea:    EA,
	pub label: String,
	pub lines: Vec<CodeLineData>,
	pub func:  FunctionData,
}

/// Data for a single line of unknown data.
#[derive(Debug, Clone)]
pub struct UnknownLineData {
	pub ea:    TextEA,
	pub bytes: String,
}

/// Data for a block of unknown data.
#[derive(Debug, Clone)]
pub struct UnknownData {
	pub lines: Vec<UnknownLineData>,
}

/// Kind of item in the code view.
#[derive(Default, Debug, Clone)]
pub enum CodeViewItem {
	BasicBlock(BasicBlockData),
	#[default]
	DataItem, // TODO
	Unknown(UnknownData),
}