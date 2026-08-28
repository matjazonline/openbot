/// Whether a fragment is being swapped into its own slot or piggybacking on another response.
///
/// Keeping this transport detail outside any one page prevents shared fragment renderers from
/// depending on the mailbox module merely because that was its first caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentSwap {
    Inline,
    OutOfBand,
}

impl FragmentSwap {
    pub(crate) fn oob_attribute(self) -> &'static str {
        match self {
            FragmentSwap::Inline => "",
            FragmentSwap::OutOfBand => r##" hx-swap-oob="outerHTML""##,
        }
    }
}
