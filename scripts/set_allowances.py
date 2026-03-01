"""
Set all required on-chain approvals for Polymarket CLOB trading.

6 transactions total:
  - USDC.e approve + CTF setApprovalForAll for CTF Exchange
  - USDC.e approve + CTF setApprovalForAll for Neg Risk CTF Exchange
  - USDC.e approve + CTF setApprovalForAll for Neg Risk Adapter

Requires: pip install web3  (tested with web3>=6.14)
Requires: POL in the EOA wallet for gas (~0.1 POL total)

Usage: POLYMARKET_PRIVATE_KEY=0x... python3 scripts/set_allowances.py
"""

import os
import sys
from web3 import Web3
from web3.constants import MAX_INT
from web3.middleware import ExtraDataToPOAMiddleware

RPC_URL = os.environ.get("POLYGON_RPC_URL", "https://1rpc.io/matic")
CHAIN_ID = 137

USDC_E = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"
CTF = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"

TARGETS = [
    ("CTF Exchange", "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E"),
    ("Neg Risk CTF Exchange", "0xC5d563A36AE78145C45a50134d48A1215220f80a"),
    ("Neg Risk Adapter", "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296"),
]

ERC20_ABI = '[{"constant":false,"inputs":[{"name":"_spender","type":"address"},{"name":"_value","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"}]'
ERC1155_ABI = '[{"inputs":[{"name":"operator","type":"address"},{"name":"approved","type":"bool"}],"name":"setApprovalForAll","outputs":[],"stateMutability":"nonpayable","type":"function"}]'


def main():
    priv_key = os.environ.get("POLYMARKET_PRIVATE_KEY")
    if not priv_key:
        print("ERROR: Set POLYMARKET_PRIVATE_KEY env var")
        sys.exit(1)

    w3 = Web3(Web3.HTTPProvider(RPC_URL))
    w3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)

    acct = w3.eth.account.from_key(priv_key)
    pub = acct.address
    print(f"Wallet: {pub}")

    bal = w3.eth.get_balance(pub)
    print(f"POL balance: {w3.from_wei(bal, 'ether')} POL")
    if bal == 0:
        print("ERROR: Need POL for gas")
        sys.exit(1)

    usdc = w3.eth.contract(address=w3.to_checksum_address(USDC_E), abi=ERC20_ABI)
    ctf = w3.eth.contract(address=w3.to_checksum_address(CTF), abi=ERC1155_ABI)

    nonce = w3.eth.get_transaction_count(pub)

    for name, addr in TARGETS:
        print(f"\n--- {name} ({addr}) ---")

        tx = usdc.functions.approve(addr, int(MAX_INT, 0)).build_transaction({
            "chainId": CHAIN_ID, "from": pub, "nonce": nonce,
            "gasPrice": w3.eth.gas_price,
        })
        signed = w3.eth.account.sign_transaction(tx, private_key=priv_key)
        h = w3.eth.send_raw_transaction(signed.raw_transaction)
        r = w3.eth.wait_for_transaction_receipt(h, 600)
        print(f"  USDC.e approve: {r.transactionHash.hex()} (status={r.status})")
        nonce += 1

        tx = ctf.functions.setApprovalForAll(addr, True).build_transaction({
            "chainId": CHAIN_ID, "from": pub, "nonce": nonce,
            "gasPrice": w3.eth.gas_price,
        })
        signed = w3.eth.account.sign_transaction(tx, private_key=priv_key)
        h = w3.eth.send_raw_transaction(signed.raw_transaction)
        r = w3.eth.wait_for_transaction_receipt(h, 600)
        print(f"  CTF setApprovalForAll: {r.transactionHash.hex()} (status={r.status})")
        nonce += 1

    print("\nAll 6 approvals complete.")


if __name__ == "__main__":
    main()
