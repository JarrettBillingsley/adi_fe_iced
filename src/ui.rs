
use std::fmt::{ Display, Formatter, Result as FmtResult };

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
	pub fn new(seg: impl Into<String>, offs: impl Into<String>) -> Self {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentData {
	pub segid:    SegId,
	pub name:     String,
	pub is_image: bool,
}

impl Display for SegmentData {
	fn fmt(&self, f: &mut Formatter) -> FmtResult {
		write!(f, "{}", self.name)
	}
}

/// Data for a single instruction operand, with an optional operand number.
#[derive(Debug, Clone)]
pub struct CodeOpData {
	pub text:  String,
	pub style: Option<PrintStyle>,
	pub opn:   Option<u8>,
}

impl CodeOpData {
	pub fn new(text: impl Into<String>, style: Option<PrintStyle>, opn: Option<u8>) -> Self {
		Self { text: text.into(), style, opn }
	}

	pub fn new_plain(text: impl Into<String>) -> Self {
		Self::new(text, None, None)
	}
}

/// Data for a single line of rendered code inside a basic block.
#[derive(Debug, Clone)]
pub struct CodeLineData {
	pub ea:       EA,
	pub text_ea:  TextEA,
	pub bytes:    String,
	pub mnemonic: String,
	pub operands: Vec<CodeOpData>,
}

/// Additional info about a function
#[derive(Debug, Clone)]
pub enum FuncDataKind {
	/// This BB is the head of a function.
	Head {
		/// If `None`, has no attributes. Otherwise, the attributes as a string.
		attrs: Option<String>,

		/// If `None`, single-entry. Otherwise, a list of entrypoint names as a string.
		entrypoints: Option<String>,
	},

	/// This BB is only a piece of a function, though it may be out-of-place (e.g. if it's part of
	/// a non-consecutive function).
	Piece,
}

/// Data about a function, to be put at the top of a function in the code listing.
#[derive(Debug, Clone)]
pub struct FuncData {
	/// Function's name.
	pub name: String,

	/// Additional info
	pub kind: FuncDataKind,
}

/// Data for a single basic block of code.
#[derive(Debug, Clone)]
pub struct BasicBlockData {
	pub ea:    EA,
	pub label: String,
	pub lines: Vec<CodeLineData>,
	pub func:  Option<FuncData>,
}

/// Data for a single line of unknown data.
#[derive(Debug, Clone)]
pub struct UnknownLineData {
	pub ea:      EA,
	pub text_ea: TextEA,
	pub bytes:   String,
}

/// Data for a block of unknown data.
#[derive(Debug, Clone)]
pub struct UnknownData {
	pub lines: Vec<UnknownLineData>,
}

/// Kind of item in the code view.
#[derive(Debug, Clone)]
pub enum CodeViewItem {
	BasicBlock(BasicBlockData),
	DataItem(EA, TextEA), // TODO
	Unknown(UnknownData),
}