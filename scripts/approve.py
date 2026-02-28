#!/usr/bin/env python3
"""
Set token approvals for Polymarket trading.
Run once per wallet. Sets max uint256 approvals so they never need refreshing.

Usage:
  source /opt/polymarket-arb/.env
  python3 scripts/approve.py
"""

import os
import sys
from web3 import Web3

RPC_URL = "https://polygon-rpc.com"
CHAIN_ID = 137

# Polymarket contract addresses (Polygon)
USDC_E = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"
CTF_CONTRACT = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"
CTF_EXCHANGE = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E"
NEG_RISK_EXCHANGE = "0xC5d563A36AE78145C45a50134d48A1215220f80a"

MAX_UINT256 = 2 ** 256 - 1

ERC20_ABI = [
    {"inputs": [{"name": "spender", "type": "address"}, {"name": "amount", "type": "uint256"}],
     "name": "approve", "outputs": [{"name": "", "type": "bool"}],
     "stateMutability": "nonpayable", "type": "function"},
    {"inputs": [{"name": "owner", "type": "address"}, {"name": "spender", "type": "address"}],
     "name": "allowance", "outputs": [{"name": "", "type": "uint256"}],
     "stateMutability": "view", "type": "function"},
]

# CTF uses setApprovalForAll (ERC1155)
ERC1155_ABI = [
    {"inputs": [{"name": "operator", "type": "address"}, {"name": "approved", "type": "bool"}],
     "name": "setApprovalForAll", "outputs": [],
     "stateMutability": "nonpayable", "type": "function"},
    {"inputs": [{"name": "account", "type": "address"}, {"name": "operator", "type": "address"}],
     "name": "isApprovedForAll", "outputs": [{"name": "", "type": "bool"}],
     "stateMutability": "view", "type": "function"},
]


def approve_erc20(w3, account, token_addr, spender, label):
    token = w3.eth.contract(address=Web3.to_checksum_address(token_addr), abi=ERC20_ABI)
    current = token.functions.allowance(account.address, Web3.to_checksum_address(spender)).call()

    if current >= MAX_UINT256 // 2:
        print(f"  {label}: already approved")
        return

    print(f"  {label}: approving...")
    tx = token.functions.approve(
        Web3.to_checksum_address(spender), MAX_UINT256
    ).build_transaction({
        "from": account.address,
        "nonce": w3.eth.get_transaction_count(account.address, "pending"),
        "gas": 100_000,
        "maxFeePerGas": w3.eth.gas_price * 2,
        "maxPriorityFeePerGas": w3.to_wei(30, "gwei"),
        "chainId": CHAIN_ID,
    })
    signed = account.sign_transaction(tx)
    tx_hash = w3.eth.send_raw_transaction(signed.raw_transaction)
    receipt = w3.eth.wait_for_transaction_receipt(tx_hash, timeout=120)
    if receipt["status"] == 1:
        print(f"  {label}: approved ✓  TX: {tx_hash.hex()}")
    else:
        print(f"  {label}: FAILED! TX: {tx_hash.hex()}")
        sys.exit(1)


def approve_erc1155(w3, account, token_addr, operator, label):
    token = w3.eth.contract(address=Web3.to_checksum_address(token_addr), abi=ERC1155_ABI)
    approved = token.functions.isApprovedForAll(
        account.address, Web3.to_checksum_address(operator)
    ).call()

    if approved:
        print(f"  {label}: already approved")
        return

    print(f"  {label}: approving...")
    tx = token.functions.setApprovalForAll(
        Web3.to_checksum_address(operator), True
    ).build_transaction({
        "from": account.address,
        "nonce": w3.eth.get_transaction_count(account.address, "pending"),
        "gas": 100_000,
        "maxFeePerGas": w3.eth.gas_price * 2,
        "maxPriorityFeePerGas": w3.to_wei(30, "gwei"),
        "chainId": CHAIN_ID,
    })
    signed = account.sign_transaction(tx)
    tx_hash = w3.eth.send_raw_transaction(signed.raw_transaction)
    receipt = w3.eth.wait_for_transaction_receipt(tx_hash, timeout=120)
    if receipt["status"] == 1:
        print(f"  {label}: approved ✓  TX: {tx_hash.hex()}")
    else:
        print(f"  {label}: FAILED! TX: {tx_hash.hex()}")
        sys.exit(1)


def main():
    key = os.environ.get("POLYMARKET_PRIVATE_KEY")
    if not key:
        print("ERROR: POLYMARKET_PRIVATE_KEY not set")
        print("Run: source /opt/polymarket-arb/.env")
        sys.exit(1)

    w3 = Web3(Web3.HTTPProvider(RPC_URL))
    if not w3.is_connected():
        print("ERROR: Cannot connect to Polygon RPC")
        sys.exit(1)

    account = w3.eth.account.from_key(key)
    print(f"Wallet: {account.address}")
    print(f"POL balance: {w3.from_wei(w3.eth.get_balance(account.address), 'ether'):.4f}")
    print()

    print("Setting approvals for Polymarket...")

    # 1. USDC.e → CTF Contract (for splitting USDC into outcome tokens)
    approve_erc20(w3, account, USDC_E, CTF_CONTRACT, "USDC.e → CTF Contract")

    # 2. CTF tokens → CTF Exchange (for standard market trading)
    approve_erc1155(w3, account, CTF_CONTRACT, CTF_EXCHANGE, "CTF → CTF Exchange")

    # 3. CTF tokens → Neg Risk Exchange (for neg-risk markets like 5-min)
    approve_erc1155(w3, account, CTF_CONTRACT, NEG_RISK_EXCHANGE, "CTF → Neg Risk Exchange")

    print()
    print("All approvals set! Ready to trade.")


if __name__ == "__main__":
    main()
