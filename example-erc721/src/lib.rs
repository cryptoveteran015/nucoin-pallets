#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use frame_support::{
    decl_error, decl_event, decl_module, decl_storage, dispatch::DispatchResult, ensure,
    traits::Get,
};
use frame_system::{self as system, ensure_root, ensure_signed};
use sp_core::U256;
use sp_runtime::RuntimeDebug;
use sp_std::prelude::*;


type TokenId = U256;

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug)]
pub struct Erc721Token {
    pub id: TokenId,
    pub metadata: Vec<u8>,
}

pub trait Trait: system::Trait {
    type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;

    type Identifier: Get<[u8; 32]>;
}

decl_event! {
    pub enum Event<T>
    where
        <T as system::Trait>::AccountId,
    {
        Minted(AccountId, TokenId),
        Transferred(AccountId, AccountId, TokenId),
        Burned(TokenId),
    }
}

decl_error! {
    pub enum Error for Module<T: Trait> {
        TokenIdDoesNotExist,
        TokenAlreadyExists,
        NotOwner,
    }
}

decl_storage! {
    trait Store for Module<T: Trait> as TokenStorage {
        Tokens get(fn tokens): map hasher(opaque_blake2_256) TokenId => Option<Erc721Token>;
        TokenOwner get(fn owner_of): map hasher(opaque_blake2_256) TokenId => Option<T::AccountId>;
        TokenCount get(fn token_count): U256 = U256::zero();
    }
}

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        type Error = Error<T>;
        fn deposit_event() = default;

        #[weight = 195_000_000]
        pub fn mint(origin, owner: T::AccountId, id: TokenId, metadata: Vec<u8>) -> DispatchResult {
            ensure_root(origin)?;

            Self::mint_token(owner, id, metadata)?;

            Ok(())
        }

        #[weight = 195_000_000]
        pub fn transfer(origin, to: T::AccountId, id: TokenId) -> DispatchResult {
            let sender = ensure_signed(origin)?;

            Self::transfer_from(sender, to, id)?;

            Ok(())
        }

        #[weight = 195_000_000]
        pub fn burn(origin, id: TokenId) -> DispatchResult {
            ensure_root(origin)?;

            let owner = Self::owner_of(id).ok_or(Error::<T>::TokenIdDoesNotExist)?;

            Self::burn_token(owner, id)?;

            Ok(())
        }
    }
}

impl<T: Trait> Module<T> {
    pub fn mint_token(owner: T::AccountId, id: TokenId, metadata: Vec<u8>) -> DispatchResult {
        ensure!(!Tokens::contains_key(id), Error::<T>::TokenAlreadyExists);

        let new_token = Erc721Token { id, metadata };

        <Tokens>::insert(&id, new_token);
        <TokenOwner<T>>::insert(&id, owner.clone());
        let new_total = <TokenCount>::get().saturating_add(U256::one());
        <TokenCount>::put(new_total);

        Self::deposit_event(RawEvent::Minted(owner, id));

        Ok(())
    }

    pub fn transfer_from(from: T::AccountId, to: T::AccountId, id: TokenId) -> DispatchResult {
        let owner = Self::owner_of(id).ok_or(Error::<T>::TokenIdDoesNotExist)?;
        ensure!(owner == from, Error::<T>::NotOwner);
        // Update owner
        <TokenOwner<T>>::insert(&id, to.clone());

        Self::deposit_event(RawEvent::Transferred(from, to, id));

        Ok(())
    }

    pub fn burn_token(from: T::AccountId, id: TokenId) -> DispatchResult {
        let owner = Self::owner_of(id).ok_or(Error::<T>::TokenIdDoesNotExist)?;
        ensure!(owner == from, Error::<T>::NotOwner);

        <Tokens>::remove(&id);
        <TokenOwner<T>>::remove(&id);
        let new_total = <TokenCount>::get().saturating_sub(U256::one());
        <TokenCount>::put(new_total);

        Self::deposit_event(RawEvent::Burned(id));

        Ok(())
    }
}
