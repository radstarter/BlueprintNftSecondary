use scrypto::prelude::*;

#[derive(NonFungibleData, ScryptoSbor)]
pub struct Badge {
  name: String,
  description: String,
  nft_address: ResourceAddress,
  nft_id: NonFungibleLocalId,
  component_address: ComponentAddress
}

#[blueprint]
mod nft_secondary_market {
  enable_method_auth! {
    roles {
      fee_owner => updatable_by: [];
    },
    methods {
      sell => PUBLIC;
      update => PUBLIC;
      cancel => PUBLIC;
      collect => PUBLIC;
      buy => PUBLIC;
      collect_fees => restrict_to: [fee_owner];
    }
  }
    
  struct NftSecondaryMarket {
    nft_vault: NonFungibleVault,
    ccy_vault: FungibleVault,
    resource_manager: ResourceManager,
    nft_address: ResourceAddress,
    ccy_address: ResourceAddress,
    badge_address: ResourceAddress,
    badges: HashMap<NonFungibleLocalId, NonFungibleLocalId>, // badge id to nft id
    offers: HashMap<NonFungibleLocalId, (NonFungibleLocalId, Decimal)>, // nft id to badge and cost
    to_collect: HashMap<NonFungibleLocalId, Decimal>, // badge id to collect amount
    component_address: ComponentAddress,
    fee_badge: ResourceAddress,
    fee_rate: Decimal,
    fee_vault: FungibleVault,
    fee_amount: Decimal
  }

  impl NftSecondaryMarket {
    pub fn instantiate_component(nft_address: ResourceAddress, ccy_address: ResourceAddress, fee_badge: ResourceAddress, fee_rate: Decimal) -> Global<NftSecondaryMarket> {
        let (address_reservation, component_address) = Runtime::allocate_component_address(NftSecondaryMarket::blueprint_id());
        let resource_manager = ResourceBuilder::new_ruid_non_fungible::<Badge>(OwnerRole::None)
                .metadata(metadata! { 
                    init { 
                        "name" => "Impahla secondary market badges", updatable; 
                        "description" => "Seller badge for secondary market", updatable; 
                        "component" => component_address, locked;
                        "tags" => vec!["utility"], updatable;
                        "icon_url" => Url::of("https://www.impahla.io/favicon.png"), updatable;
                        "info_url" => Url::of("https://www.impahla.io/"), updatable;
                    }
                })
                .mint_roles(mint_roles! (
                    minter => rule!(require(global_caller(component_address))); 
                    minter_updater => rule!(deny_all);
                ))
                .burn_roles(burn_roles! {
                    burner => rule!(require(global_caller(component_address))); 
                    burner_updater => rule!(deny_all);
                })
                .create_with_no_initial_supply();
        let component = Self {
                nft_vault: NonFungibleVault::new(nft_address),
                ccy_vault: FungibleVault::new(ccy_address),
                resource_manager: resource_manager,
                nft_address: nft_address,
                ccy_address: ccy_address,
                badge_address: resource_manager.address(),
                badges: HashMap::new(),
                offers: HashMap::new(),
                to_collect: HashMap::new(),
                component_address: component_address,
                fee_badge: fee_badge,
                fee_rate: fee_rate,
                fee_vault: FungibleVault::new(ccy_address),
                fee_amount: dec!(0),
            }.instantiate();
        component.prepare_to_globalize(OwnerRole::None)
                 .roles(roles! {
                   fee_owner => rule!(require(fee_badge));
                 })
                 .with_address(address_reservation)
                 .globalize()
    }
    
    pub fn sell(&mut self, nft_bucket: NonFungibleBucket, cost: Decimal) -> NonFungibleBucket {
        assert!(cost >= Decimal::zero(), "the cost should be positive");
        assert!(nft_bucket.resource_address() == self.nft_address, "wrong nft ressource");
        let nft_id = nft_bucket.non_fungible_local_id();
        let badge_bucket = self.resource_manager.mint_ruid_non_fungible(Badge {
            name: String::from("impahla seller badge"),
            description: String::from("this badge allow you to interact with your offer in the secondary market"),
            nft_address: self.nft_address,
            nft_id: nft_id.clone(),
            component_address: self.component_address
          }).as_non_fungible();
        let badge_id = badge_bucket.non_fungible_local_id();
        self.badges.insert(badge_id.clone(), nft_id.clone());
        self.offers.insert(nft_id, (badge_id, cost));
        self.nft_vault.put(nft_bucket);
        badge_bucket
    }
    
    pub fn update(&mut self, badge_bucket: NonFungibleBucket, cost: Decimal) -> NonFungibleBucket {
        assert!(cost >= Decimal::zero(), "the cost should be positive");
        assert!(badge_bucket.resource_address() == self.badge_address, "wrong badge ressource");
        let badge_id = badge_bucket.non_fungible_local_id();
        let nft_id = self.badges.get(&badge_id).expect("invalid badge");
        self.offers.remove(&nft_id).expect("already cancelled or bought");
        self.offers.insert(nft_id.clone(), (badge_id, cost));
        badge_bucket
    }
    
    pub fn cancel(&mut self, badge_bucket: NonFungibleBucket) -> NonFungibleBucket {
        assert!(badge_bucket.resource_address() == self.badge_address, "wrong badge ressource");
        let badge_id = badge_bucket.non_fungible_local_id();
        let nft_id = self.badges.remove(&badge_id).expect("invalid badge");
        self.offers.remove(&nft_id).expect("already cancelled or bought");
        badge_bucket.burn();
        self.nft_vault.take_non_fungible(&nft_id)
    }
    
    pub fn collect(&mut self, badge_bucket: NonFungibleBucket) -> FungibleBucket {
        assert!(badge_bucket.resource_address() == self.badge_address, "wrong badge ressource");
        let badge_id = badge_bucket.non_fungible_local_id();
        let _nft_id = self.badges.remove(&badge_id).expect("invalid badge");
        let cost = self.to_collect.remove(&badge_id).expect("already collected");
        badge_bucket.burn();
        self.ccy_vault.take(cost)
    }
    
    pub fn buy(&mut self, nft_id: NonFungibleLocalId, mut ccy_bucket: FungibleBucket) -> (NonFungibleBucket, FungibleBucket) {
        let (badge_id, cost) = self.offers.remove(&nft_id).expect("invalid badge");
        
        let mut bucket = ccy_bucket.take(cost);
        self.fee_vault.put(bucket.take(cost*self.fee_rate));
        self.fee_amount = self.fee_vault.amount();
        self.to_collect.insert(badge_id, bucket.amount());
        self.ccy_vault.put(bucket);
        let nft_bucket = self.nft_vault.take_non_fungible(&nft_id);
        (nft_bucket, ccy_bucket)
    }
    
    pub fn collect_fees(&mut self) -> FungibleBucket {
        self.fee_amount = dec!(0);
        self.fee_vault.take_all()
    }
  }
}
