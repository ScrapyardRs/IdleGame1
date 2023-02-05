use mcprotocol::common::chat::Chat;
use mcprotocol::msg;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(usize)]
pub enum Rank {
    Default = 0,
    Staff = 1,
    Owner = 2,
}

impl Rank {
    pub fn format_name(self, name: String) -> Chat {
        match self {
            Rank::Default => msg!(name, "#162c4f"),
            Rank::Staff => msg!(format!("[Staff] {}", name), "#2f803d"),
            Rank::Owner => msg!(format!("[Owner] {}", name), "#752916"),
        }
        .into()
    }
}
