#!/usr/bin/env python3
"""
Transfer all USDC and POL from the old (compromised) wallet to the new one.
The old key is already exposed — this script drains it to the new address.

Usage: python3 scripts/transfer_funds.py
"""

from web3 import Web3

RPC = "https://polygon-mainnet.g.alchemy.com/v2/RqOrdvCT92g7AqRCY7Yp5"
OLD_KEY = "0xceafcb4c7f4df1b7212987eb1fccfbece0766d7d0d82f67a12f6c21b9e9c9d7f"
NEW_ADDR = "0x82315dB84Efe51F8aD3D7dFa35D87E0e342E9f86"
USDC = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"
CHAIN_ID = 137

ERC20_ABI = [
    {"inputs": [{"name": "to", "type": "address"}, {"name": "amount", "type": "uint256"}],
     "name": "transfer", "outputs": [{"name": "", "type": "bool"}],
     "stateMutability": "nonpayable", "type": "function"},
    {"inputs": [{"name": "account", "type": "address"}],
     "name": "balanceOf", "outputs": [{"name": "", "type": "uint256"}],
     "stateMutability": "view", "type": "function"},
]

w3 = Web3(Web3.HTTPProvider(RPC))
old = w3.eth.account.from_key(OLD_KEY)
new = Web3.to_checksum_address(NEW_ADDR)

print(f"Old wallet: {old.address}")
print(f"New wallet: {new}")
print()

# Transfer USDC
usdc = w3.eth.contract(address=Web3.to_checksum_address(USDC), abi=ERC20_ABI)
usdc_bal = usdc.functions.balanceOf(old.address).call()
print(f"USDC balance: {usdc_bal / 1e6:.2f}")

if usdc_bal > 0:
    gas_price = w3.eth.gas_price
    tx = usdc.functions.transfer(new, usdc_bal).build_transaction({
        "from": old.address,
        "nonce": w3.eth.get_transaction_count(old.address, "pending"),
        "gas": 100_000,
        "maxFeePerGas": gas_price * 2,
        "maxPriorityFeePerGas": w3.to_wei(30, "gwei"),
        "chainId": CHAIN_ID,
    })
    signed = old.sign_transaction(tx)
    tx_hash = w3.eth.send_raw_transaction(signed.raw_transaction)
    receipt = w3.eth.wait_for_transaction_receipt(tx_hash, timeout=120)
    print(f"USDC transfer: {'OK' if receipt['status'] == 1 else 'FAILED'} TX: {tx_hash.hex()}")
else:
    print("No USDC to transfer")

# Transfer POL (leave enough for this tx's gas)
pol_bal = w3.eth.get_balance(old.address)
print(f"POL balance: {w3.from_wei(pol_bal, 'ether'):.4f}")

gas_cost = 21000 * w3.eth.gas_price * 3  # buffer for gas
transfer_amount = pol_bal - gas_cost

if transfer_amount > 0:
    tx = {
        "from": old.address,
        "to": new,
        "value": transfer_amount,
        "nonce": w3.eth.get_transaction_count(old.address, "pending"),
        "gas": 21000,
        "maxFeePerGas": w3.eth.gas_price * 2,
        "maxPriorityFeePerGas": w3.to_wei(30, "gwei"),
        "chainId": CHAIN_ID,
    }
    signed = old.sign_transaction(tx)
    tx_hash = w3.eth.send_raw_transaction(signed.raw_transaction)
    receipt = w3.eth.wait_for_transaction_receipt(tx_hash, timeout=120)
    print(f"POL transfer: {'OK' if receipt['status'] == 1 else 'FAILED'} TX: {tx_hash.hex()}")
else:
    print("Not enough POL to transfer after gas")

print()
print("Done. Old wallet drained. Now run approvals for the new wallet:")
print("  source /opt/polymarket-arb/.env")
print("  python3 scripts/approve.py")
print("  ./scripts/start.sh daemon")
