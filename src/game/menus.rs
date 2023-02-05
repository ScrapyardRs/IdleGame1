use crate::game::blocks::{PlayerBlockData, GLOBAL_BLOCK_REGISTRY};
use mcprotocol::msg;
use shovel::inventory::Menu;

pub fn mined_statistics_page<C: Send + Sync>(data: &PlayerBlockData) -> Menu<C> {
    let mut menu = Menu::from_rows(1, msg!("Mining Statistics", "aqua").bold(true), 1);
    let mut counter_x = 0;
    let mut counter_y = 0;
    for (ordinal, count) in data.mined_blocks.iter().enumerate() {
        if let Some(block) = GLOBAL_BLOCK_REGISTRY.search_by_ordinal(ordinal) {
            let item = block.create_item(*count);
            menu.set_item_unaware(counter_x, counter_y, Some(item));
            counter_x += 1;
            if counter_x == 9 {
                counter_y += 1;
                counter_x = 0;
            }
        }
    }
    menu
}
