# nft-collection

NFT Collection - Secondary Market

# Description

Blueprint for secondary market for NFT Collection

# Operation available

- `instantiate(nft addr, ccy addr)`: create a new secondary market for a targeted NFT collection, specify the currrency to be used (ex: XRD)
- `sell(nft, cost) -> badge`: send the NFT to be sold at the `cost` price, receive a `badge` in exchange
- `update(badge, cost)`: update the `cost`
- `cancel(badge) -> nft`: cancel the sale, retrieve the NFT and burn the `badge`
- `collect(badge) -> ccy`: once the NFT is sold, collect the CCY and burn the `badge`
- `buy(id, ccy) -> nft`: buy the NFT
