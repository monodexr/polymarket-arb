pub mod book;
pub mod discovery;
pub mod fair_value;

use book::BookState;
use discovery::MarketState;

pub fn spawn_clob_ws(market_state: MarketState) -> BookState {
    book::spawn(market_state)
}
