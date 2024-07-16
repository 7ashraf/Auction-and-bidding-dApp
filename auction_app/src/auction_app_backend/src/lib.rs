#[macro_use]
extern crate serde;
use candid::{Decode, Encode, Principal};
use ic_cdk::api::time;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};

// Define the memory types
type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

// Define the Item struct
#[derive(candid::CandidType, Clone, Serialize, Deserialize)]
struct Item {
    id: u64,
    title: String,
    description: String,
    starting_bid: u64,
    highest_bid: Option<u64>,
    highest_bidder: Option<Principal>,
    owner: Principal,
    new_owner: Option<Principal>,
    created_at: u64,
    updated_at: Option<u64>,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct MyPrincipal(Option<Principal>);
impl Storable for MyPrincipal {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for MyPrincipal {
    const MAX_SIZE: u32 = 1024;
    const IS_FIXED_SIZE: bool = false;
}
// Implement Storable for Item
impl Storable for Item {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

// Implement BoundedStorable for Item
impl BoundedStorable for Item {
    const MAX_SIZE: u32 = 1024;
    const IS_FIXED_SIZE: bool = false;
}


// Define the Bid struct
#[derive(candid::CandidType, Clone, Serialize, Deserialize)]
struct Bid {
    item_id: u64,
    bidder: Principal,
    amount: u64,
    timestamp: u64,
}

// Implement Storable for Bid
impl Storable for Bid {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

// Implement BoundedStorable for Bid
impl BoundedStorable for Bid {
    const MAX_SIZE: u32 = 256;
    const IS_FIXED_SIZE: bool = false;
}

// Define the global variables
thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    static ITEM_STORAGE: RefCell<StableBTreeMap<u64, Item, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
    ));

    static BID_STORAGE: RefCell<StableBTreeMap<(u64, MyPrincipal), Bid, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2)))
    ));
}

// Define the payload for creating or updating an item
#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct ItemPayload {
    title: String,
    description: String,
    starting_bid: u64,
}

// Add a new item
#[ic_cdk::update]
fn add_item(payload: ItemPayload) -> Option<Item> {
    let id = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("Cannot increment id counter");
    let item = Item {
        id,
        title: payload.title,
        description: payload.description,
        starting_bid: payload.starting_bid,
        highest_bid: None,
        highest_bidder: None,
        owner: ic_cdk::caller(),
        new_owner: None,
        created_at: time(),
        updated_at: None,
    };
    do_insert_item(&item);
    Some(item)
}

// Bid for an item
#[ic_cdk::update]
fn bid_for_item(id: u64, bid_amount: u64) -> Result<Item, Error> {
    ITEM_STORAGE.with(|item_storage| {
        let mut item_storage = item_storage.borrow_mut();
        if let Some(mut item) = item_storage.get(&id) {
            if bid_amount > item.highest_bid.unwrap_or(item.starting_bid) {
                let bid = Bid {
                    item_id: id,
                    bidder: ic_cdk::caller(),
                    amount: bid_amount,
                    timestamp: time(),
                };
                BID_STORAGE.with(|bid_storage| {
                    bid_storage.borrow_mut().insert((id, MyPrincipal(Some(ic_cdk::caller()))), bid);
                });
                item.highest_bid = Some(bid_amount);
                item.highest_bidder = Some(ic_cdk::caller());
                item.updated_at = Some(time());
                item_storage.insert(id, item.clone());
                Ok(item)
            } else {
                Err(Error::InvalidBid {
                    msg: format!("Bid amount is too low. Current highest bid: {:?}", item.highest_bid),
                })
            }
        } else {
            Err(Error::NotFound {
                msg: format!("Item with id={} not found", id),
            })
        }
    })
}

// Update an item listing
#[ic_cdk::update]
fn update_item(id: u64, payload: ItemPayload) -> Result<Item, Error> {
    ITEM_STORAGE.with(|item_storage| {
        let mut item_storage = item_storage.borrow_mut();
        if let Some(mut item) = item_storage.get(&id) {
            if item.owner == ic_cdk::caller() {
                item.title = payload.title;
                item.description = payload.description;
                item.starting_bid = payload.starting_bid;
                item.updated_at = Some(time());
                item_storage.insert(id, item.clone());
                Ok(item)
            } else {
                Err(Error::Unauthorized {
                    msg: "Only the owner can update the item".to_string(),
                })
            }
        } else {
            Err(Error::NotFound {
                msg: format!("Item with id={} not found", id),
            })
        }
    })
}

// Stop an item listing
#[ic_cdk::update]
fn stop_item_listing(id: u64) -> Result<Item, Error> {
    ITEM_STORAGE.with(|item_storage| {
        let mut item_storage = item_storage.borrow_mut();
        if let Some(mut item) = item_storage.get(&id) {
            if item.owner == ic_cdk::caller() {
                item.new_owner = item.highest_bidder.clone();
                item.updated_at = Some(time());
                item_storage.insert(id, item.clone());
                Ok(item)
            } else {
                Err(Error::Unauthorized {
                    msg: "Only the owner can stop the item listing".to_string(),
                })
            }
        } else {
            Err(Error::NotFound {
                msg: format!("Item with id={} not found", id),
            })
        }
    })
}

// Helper method to perform item insert
fn do_insert_item(item: &Item) {
    ITEM_STORAGE.with(|item_storage| item_storage.borrow_mut().insert(item.id, item.clone()));
}

// Retrieve an item by ID
// Retrieve an item by ID
#[ic_cdk::query]
fn get_item(id: u64) -> Result<Item, Error> {
    ITEM_STORAGE.with(|item_storage| {
        match item_storage.borrow().get(&id) {
            Some(item) => Ok(item.clone()),
            None => Err(Error::NotFound {
                msg: format!("Item with id={} not found", id),
            }),
        }
    })
}


// Retrieve all items
#[ic_cdk::query]
fn get_all_items() -> Vec<Item> {
    ITEM_STORAGE.with(|item_storage| {
        item_storage
            .borrow()
            .iter()
            .map(|(_, item)| item.clone())
            .collect()
    })
}

// Retrieve the length of items listed
#[ic_cdk::query]
fn get_items_length() -> u64 {
    ITEM_STORAGE.with(|item_storage| item_storage.borrow().len() as u64)
}

// Retrieve the item sold for the most
#[ic_cdk::query]
fn get_item_sold_for_most() -> Option<Item> {
    ITEM_STORAGE.with(|item_storage| {
        item_storage
            .borrow()
            .iter()
            .filter(|(_, item)| item.new_owner.is_some())
            .max_by_key(|(_, item)| item.highest_bid.unwrap_or(0))
            .map(|(_, item)| item.clone())
    })
}

// Retrieve the item that has been bid on the most
#[ic_cdk::query]
fn get_item_bid_on_most() -> Option<Item> {
    ITEM_STORAGE.with(|item_storage| {
        item_storage
            .borrow()
            .iter()
            .max_by_key(|(_, item)| item.highest_bid.unwrap_or(0))
            .map(|(_, item)| item.clone())
    })
}

// Define the Error enum
#[derive(candid::CandidType, Deserialize, Serialize)]
enum Error {
    NotFound { msg: String },
    InvalidBid { msg: String },
    Unauthorized { msg: String },
}

// Need this to generate candid
ic_cdk::export_candid!();
