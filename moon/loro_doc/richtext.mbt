///|
pub enum ExpandType {
  Before
  After
  Both
  None_
} derive(Eq, Show)

fn ExpandType::expand_before(self : ExpandType) -> Bool {
  match self {
    ExpandType::Before => true
    ExpandType::Both => true
    _ => false
  }
}

fn ExpandType::expand_after(self : ExpandType) -> Bool {
  match self {
    ExpandType::After => true
    ExpandType::Both => true
    _ => false
  }
}

///|
pub fn ExpandType::reverse(self : ExpandType) -> ExpandType {
  match self {
    ExpandType::Before => ExpandType::Before
    ExpandType::After => ExpandType::After
    ExpandType::Both => ExpandType::None_
    ExpandType::None_ => ExpandType::Both
  }
}

const TEXT_STYLE_EXPAND_BEFORE_MASK : UInt = 0b0000_0010U
const TEXT_STYLE_EXPAND_AFTER_MASK : UInt = 0b0000_0100U
const TEXT_STYLE_ALIVE_MASK : UInt = 0b1000_0000U

///|
pub fn text_style_info_flag_from_expand(expand : ExpandType) -> Byte {
  let mut data : UInt = TEXT_STYLE_ALIVE_MASK
  if expand.expand_before() {
    data = data | TEXT_STYLE_EXPAND_BEFORE_MASK
  }
  if expand.expand_after() {
    data = data | TEXT_STYLE_EXPAND_AFTER_MASK
  }
  (data & 0xFFU).to_byte()
}

fn text_style_info_expand_before(info : Byte) -> Bool {
  (info.to_uint() & TEXT_STYLE_EXPAND_BEFORE_MASK) != 0U
}

fn text_style_info_expand_after(info : Byte) -> Bool {
  (info.to_uint() & TEXT_STYLE_EXPAND_AFTER_MASK) != 0U
}

///|
pub fn text_style_info_prefer_insert_before(info : Byte, anchor_type : TextAnchorType) -> Bool {
  match anchor_type {
    TextAnchorType::Start =>
      // If we need to expand the style, the new text should be inserted **after** the start anchor.
      !text_style_info_expand_before(info)
    TextAnchorType::End =>
      // If we need to expand the style, the new text should be inserted **before** the end anchor.
      text_style_info_expand_after(info)
  }
}
