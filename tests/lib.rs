use radix_engine::transaction::{TransactionReceipt, BalanceChange, CommitResult};
use radix_engine::types::ManifestSbor;
use scrypto::prelude::*;
use scrypto_unit::*;
use transaction::builder::ManifestBuilder;
use transaction::signing::*;
use transaction::model::{TransactionManifestV1};
use transaction::signing::secp256k1::Secp256k1PrivateKey;

type Actor = (Secp256k1PublicKey, Secp256k1PrivateKey, ComponentAddress);

struct TestEnv {
    runner: DefaultTestRunner,
    instance: ComponentAddress,
    nft_addr: ResourceAddress,
    badge_addr: ResourceAddress
}

#[derive(ScryptoSbor, NonFungibleData, ManifestSbor)]
struct EmptyNonFungibleData {}

fn create_non_fungible_tokens<'a>(
    runner: &mut DefaultTestRunner,
    owner: &Actor,
    ids: impl Iterator<Item = &'a u64>
) -> ResourceAddress {
    let mut entries = BTreeMap::new();
    ids.for_each(|i| -> () { entries.insert(NonFungibleLocalId::integer(*i), EmptyNonFungibleData {}); });

    let transaction = ManifestBuilder::new()
        .create_non_fungible_resource(OwnerRole::None, NonFungibleIdType::Integer, false, NonFungibleResourceRoles::default(), metadata!(), Some(entries))
        .deposit_batch(owner.2)
        .build();
    let receipt = runner.execute_manifest_ignoring_fee(transaction, vec![NonFungibleGlobalId::from_public_key(&owner.0)]);
    receipt.expect_commit_success();
    return receipt.expect_commit(true).new_resource_addresses()[0];
}

fn transfert_nft<'a>(
    runner: &mut DefaultTestRunner,
    addr: ResourceAddress,
    ids: impl Iterator<Item = &'a u64>,
    src: &Actor,
    dest: &Actor) {
    
    let mut entries = BTreeSet::new();
    ids.for_each(|i| -> () { entries.insert(NonFungibleLocalId::integer(*i)); });

    let transaction = ManifestBuilder::new()
        .withdraw_non_fungibles_from_account(src.2, addr, entries)
        .deposit_batch(dest.2)
        .build();
    let receipt = runner.execute_manifest_ignoring_fee(transaction, vec![
      NonFungibleGlobalId::from_public_key(&src.0),
      NonFungibleGlobalId::from_public_key(&dest.0)
    ]);
    receipt.expect_commit_success();
}

fn create_fungible_tokens(
    runner: &mut DefaultTestRunner,
    owner: &Actor,
    nb: Decimal
) -> ResourceAddress {
    let transaction = ManifestBuilder::new()
        .new_token_fixed(OwnerRole::None, metadata!(), nb)
        .deposit_batch(owner.2)
        .build();
    let receipt = runner.execute_manifest_ignoring_fee(transaction, vec![NonFungibleGlobalId::from_public_key(&owner.0)]);
    receipt.expect_commit_success();
    return receipt.expect_commit(true).new_resource_addresses()[0];
}

impl TestEnv {
    fn new(fee_rate: Decimal) -> (
        TestEnv,
        Actor,      // seller: key, account
        Vec<Actor>, // buyers: key, account
        ResourceAddress, // NFT address
        ResourceAddress  // Fee owner Badge
    ) {
        let mut runner = TestRunnerBuilder::new().without_trace().build();
        let seller = runner.new_allocated_account();
        let nft_addr = create_non_fungible_tokens(&mut runner, &seller, [1,2,3].iter());
        let buyers: Vec<Actor> = (0..3).map(|_| runner.new_allocated_account()).collect();
        let package = runner.compile_and_publish(this_package!());
        
        let fee_badge = create_fungible_tokens(&mut runner, &seller, dec!(1));
        
        let transaction = ManifestBuilder::new()
            .call_function(package, "NftSecondaryMarket", "instantiate_component", manifest_args!(nft_addr, XRD, fee_badge, fee_rate))
            .deposit_batch(seller.2)
            .build();
        let receipt = runner.execute_manifest_ignoring_fee(transaction, vec![NonFungibleGlobalId::from_public_key(&seller.0)]);
        println!("{:?}\n", receipt);
        receipt.expect_commit_success();
        let result = &receipt.expect_commit(true);
        //println!("{:?}\n", result);
        let instance = result.new_component_addresses()[0];
        let badge_addr = result.new_resource_addresses()[0];
        (
            TestEnv {
                runner,
                instance,
                nft_addr,
                badge_addr,
            },
            seller,
            buyers,
            nft_addr,
            fee_badge
        )
    }
    
    fn execute(&mut self, transaction: TransactionManifestV1, actor: &Actor) -> TransactionReceipt {
      self.runner.execute_manifest_ignoring_fee(transaction, vec![NonFungibleGlobalId::from_public_key(&actor.0)])
    }
    
    fn sell(&mut self, actor: &Actor, id: &NonFungibleLocalId, cost: Decimal) -> NonFungibleLocalId {
        let transaction = ManifestBuilder::new()
            .withdraw_non_fungibles_from_account(actor.2, self.nft_addr, BTreeSet::from([id.clone()]))
            .take_non_fungibles_from_worktop(self.nft_addr, BTreeSet::from([id.clone()]), "nft")
            .call_method_with_name_lookup(self.instance, "sell", |lookup| (
                  lookup.bucket("nft"),
                        cost
                )
              )
            .deposit_batch(actor.2)
            .build();
        let receipt = self.execute(transaction, actor);
        println!("{:?}\n", receipt);
        let result = receipt.expect_commit_success();
        //scrypto_decode(&result[2].as_vec()).unwrap()
        //IndexMap<GlobalAddress, IndexMap<ResourceAddress, BalanceChange>>
        println!("vault_balance_changes: {:?}\n", result.vault_balance_changes());
        let actor_account = actor.2.as_node_id();
        let changes = self.runner.sum_descendant_balance_changes(result, actor_account);
        let (badge_addr, val) = changes.iter().filter(|(k, _)| **k == self.badge_addr).next().unwrap();
        println!("id: {:?} {:?}\n", id, val);
        val.clone().added_non_fungibles().iter().next().unwrap().clone()
    }
    
    fn collect(&mut self, actor: &Actor, badge: &NonFungibleLocalId) -> CommitResult {
        let transaction = ManifestBuilder::new()
            .withdraw_non_fungibles_from_account(actor.2, self.badge_addr, BTreeSet::from([badge.clone()]))
            .take_non_fungibles_from_worktop(self.badge_addr, BTreeSet::from([badge.clone()]), "badge")
            .call_method_with_name_lookup(self.instance, "collect", |lookup| (
                  lookup.bucket("badge"),
                )
              )
            .deposit_batch(actor.2)
            .build();
        let receipt = self.execute(transaction, actor);
        println!("{:?}\n", receipt);
        receipt.expect_commit_success().clone()
    }
    
    fn buy_intern(&mut self, actor: &Actor, id: &NonFungibleLocalId, amount: Decimal, should_fail: bool) {
        let transaction = ManifestBuilder::new()
            .withdraw_from_account(actor.2, XRD, amount)
            .take_all_from_worktop(XRD, "ccy")
            .call_method_with_name_lookup(self.instance, "buy", |lookup| (
                  id.clone(),
                  lookup.bucket("ccy")
                )
              )
            .deposit_batch(actor.2)
            .build();
        let receipt = self.execute(transaction, actor);
        println!("{:?}\n", receipt);
        if should_fail {
          receipt.expect_commit_failure();
        } else {
          receipt.expect_commit_success();
        }
    }
    
    fn buy(&mut self, actor: &Actor, id: &NonFungibleLocalId, amount: Decimal) {
        self.buy_intern(actor, id, amount, false);
    }
    
    fn buy_fail(&mut self, actor: &Actor, id: &NonFungibleLocalId, amount: Decimal) {
        self.buy_intern(actor, id, amount, true);
    }
    
    fn cancel(&mut self, actor: &Actor, badge: &NonFungibleLocalId) {
        let transaction = ManifestBuilder::new()
            .withdraw_non_fungibles_from_account(actor.2, self.badge_addr, BTreeSet::from([badge.clone()]))
            .take_non_fungibles_from_worktop(self.badge_addr, BTreeSet::from([badge.clone()]), "badge")
            .call_method_with_name_lookup(self.instance, "cancel", |lookup| (
                  lookup.bucket("badge"),
                )
              )
            .deposit_batch(actor.2)
            .build();
        let receipt = self.execute(transaction, actor);
        println!("{:?}\n", receipt);
        receipt.expect_commit_success();
    }
    
    fn update(&mut self, actor: &Actor, badge: &NonFungibleLocalId, cost: Decimal) {
        let transaction = ManifestBuilder::new()
            .withdraw_non_fungibles_from_account(actor.2, self.badge_addr, BTreeSet::from([badge.clone()]))
            .take_non_fungibles_from_worktop(self.badge_addr, BTreeSet::from([badge.clone()]), "badge")
            .call_method_with_name_lookup(self.instance, "update", |lookup| (
                  lookup.bucket("badge"),
                  cost
                )
              )
            .deposit_batch(actor.2)
            .build();
        let receipt = self.execute(transaction, actor);
        println!("{:?}\n", receipt);
        receipt.expect_commit_success();
    }
    
    fn collect_fees(&mut self, actor: &Actor, fee_badge: ResourceAddress) -> CommitResult {
        let transaction = ManifestBuilder::new()
            .create_proof_from_account_of_amount(actor.2, fee_badge, dec!(1))
            .call_method(self.instance,"collect_fees", manifest_args!())
            .deposit_batch(actor.2)
            .build();
        let receipt = self.execute(transaction, actor);
        receipt.expect_commit_success().clone()
    }
    
    fn check_balance_change(&mut self, commit_result: &CommitResult, actor: &Actor, ressource: ResourceAddress, exp_amount: Decimal) {
        let balance_changes = commit_result.vault_balance_changes();
        for (vault_id, (resource, delta)) in balance_changes.iter() {
            println!("Vault: {:?}\n   Resource: {:?}\n   Change: {}",
                vault_id,
                resource,
                match delta {
                    BalanceChange::Fungible(d) => format!("{}", d),
                    BalanceChange::NonFungible { added, removed } => { format!("+{:?}, -{:?}", added, removed) }
                }
            );
        }
        let balances = self.runner.sum_descendant_balance_changes(commit_result, actor.2.as_node_id());
        let zeroBalance = BalanceChange::Fungible(dec!(0));
        let balance = balances.get(&ressource).unwrap_or(&zeroBalance);
        let amount = match balance {
                        BalanceChange::Fungible(d) => d,
                        _ => panic!("expect fungible")
                     };
        println!("{:?}\n", self.runner.sum_descendant_balance_changes(commit_result, actor.2.as_node_id()));
        assert_eq!(*amount, exp_amount);
    }
}

#[test]
fn test_instantiate() {
    let (mut env, owner, _, nft_addr, _) = TestEnv::new(dec!(0));
}

#[test]
fn test_sell() {
    let (mut env, owner, _, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    env.sell(&owner, &id, dec!(5));
}

#[test]
fn test_sell_buy() {
    let (mut env, owner, buyers, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.buy(&buyers[0], &id, dec!(5));
}

#[test]
fn test_sell_buy_collect() {
    let (mut env, owner, buyers, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.buy(&buyers[0], &id, dec!(5));
    let result = env.collect(&owner, &badge);
    env.check_balance_change(&result, &owner, XRD, dec!(5));
}

#[test]
fn test_sell_cancel() {
    let (mut env, owner, _, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.cancel(&owner, &badge);
}

#[test]
fn test_sell_cancel_buy_fail() {
    let (mut env, owner, buyers, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.cancel(&owner, &badge);
    env.buy_fail(&buyers[0], &id, dec!(5));
}

#[test]
fn test_sell_update_buy_collect() {
    let (mut env, owner, buyers, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.update(&owner, &badge, dec!(4));
    env.buy(&buyers[0], &id, dec!(4));
}

#[test]
fn test_sell_buy_low_fail() {
    let (mut env, owner, buyers, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.update(&owner, &badge, dec!(6));
    env.buy_fail(&buyers[0], &id, dec!(5));
}

#[test]
fn test_sell_buy_sell_collect() {
    let (mut env, owner, buyers, nft_addr, _) = TestEnv::new(dec!(0));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(5));
    env.buy(&buyers[0], &id, dec!(5));
    let badge2 = env.sell(&buyers[0], &id, dec!(10));
    let result = env.collect(&owner, &badge);
    env.check_balance_change(&result, &owner, XRD, dec!(5));
}

#[test]
fn test_fee() {
    let (mut env, owner, buyers, nft_addr, fee_badge) = TestEnv::new(dec!(0.25));
    let id = NonFungibleLocalId::integer(1);
    let badge = env.sell(&owner, &id, dec!(20));
    env.buy(&buyers[0], &id, dec!(20));
    let badge2 = env.sell(&buyers[0], &id, dec!(40));
    env.buy(&buyers[1], &id, dec!(40));
    let result_collect = env.collect(&owner, &badge);
    env.check_balance_change(&result_collect, &owner, XRD, dec!(15));
    let result_collect2 = env.collect(&buyers[0], &badge2);
    env.check_balance_change(&result_collect2, &buyers[0], XRD, dec!(30));
    let result_fee = env.collect_fees(&owner, fee_badge);
    env.check_balance_change(&result_fee, &owner, XRD, dec!(15));
}

