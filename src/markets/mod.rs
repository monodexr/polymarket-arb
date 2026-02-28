pub mod book;
pub mod discovery;
pub mod fair_value;

use book::BookRx;
use discovery::MarketStateRx;

pub fn spawn_clob_ws(market_rx: MarketStateRx) -> BookRx {
    book::spawn(market_rx)
}
