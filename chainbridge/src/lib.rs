#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
    decl_error, decl_event, decl_module, decl_storage,
    dispatch::DispatchResult,
    ensure,
    traits::{EnsureOrigin, Get},
    weights::{GetDispatchInfo, Pays},
    Parameter,
};

use frame_system::{self as system, ensure_root, ensure_signed};
use sp_core::U256;
use sp_runtime::traits::{AccountIdConversion, Dispatchable};
use sp_runtime::{ModuleId, RuntimeDebug};
use sp_std::prelude::*;

use codec::{Decode, Encode, EncodeLike};


const DEFAULT_RELAYER_THRESHOLD: u32 = 1;
const MODULE_ID: ModuleId = ModuleId(*b"cb/bridg");

pub type ChainId = u8;
pub type DepositNonce = u64;
pub type ResourceId = [u8; 32];

pub fn derive_resource_id(chain: u8, id: &[u8]) -> ResourceId {
    let mut r_id: ResourceId = [0; 32];
    r_id[31] = chain;
    let range = if id.len() > 31 { 31 } else { id.len() };
    for i in 0..range {
        r_id[30 - i] = id[range - 1 - i];
    }
    return r_id;
}

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug)]
pub enum ProposalStatus {
    Initiated,
    Approved,
    Rejected,
}

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug)]
pub struct ProposalVotes<AccountId, BlockNumber> {
    pub votes_for: Vec<AccountId>,
    pub votes_against: Vec<AccountId>,
    pub status: ProposalStatus,
    pub expiry: BlockNumber,
}

impl<A: PartialEq, B: PartialOrd + Default> ProposalVotes<A, B> {
    fn try_to_complete(&mut self, threshold: u32, total: u32) -> ProposalStatus {
        if self.votes_for.len() >= threshold as usize {
            self.status = ProposalStatus::Approved;
            ProposalStatus::Approved
        } else if total >= threshold && self.votes_against.len() as u32 + threshold > total {
            self.status = ProposalStatus::Rejected;
            ProposalStatus::Rejected
        } else {
            ProposalStatus::Initiated
        }
    }

    fn is_complete(&self) -> bool {
        self.status != ProposalStatus::Initiated
    }

    fn has_voted(&self, who: &A) -> bool {
        self.votes_for.contains(&who) || self.votes_against.contains(&who)
    }

    fn is_expired(&self, now: B) -> bool {
        self.expiry <= now
    }
}

impl<AccountId, BlockNumber: Default> Default for ProposalVotes<AccountId, BlockNumber> {
    fn default() -> Self {
        Self {
            votes_for: vec![],
            votes_against: vec![],
            status: ProposalStatus::Initiated,
            expiry: BlockNumber::default(),
        }
    }
}

pub trait Trait: system::Trait {
    type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;
    type AdminOrigin: EnsureOrigin<Self::Origin>;
    type Proposal: Parameter + Dispatchable<Origin = Self::Origin> + EncodeLike + GetDispatchInfo;
    type ChainId: Get<ChainId>;

    type ProposalLifetime: Get<Self::BlockNumber>;
}

decl_event! {
    pub enum Event<T> where <T as frame_system::Trait>::AccountId {
        RelayerThresholdChanged(u32),
        ChainWhitelisted(ChainId),
        RelayerAdded(AccountId),
        RelayerRemoved(AccountId),
        FungibleTransfer(ChainId, DepositNonce, ResourceId, U256, Vec<u8>),
        NonFungibleTransfer(ChainId, DepositNonce, ResourceId, Vec<u8>, Vec<u8>, Vec<u8>),
        GenericTransfer(ChainId, DepositNonce, ResourceId, Vec<u8>),
        VoteFor(ChainId, DepositNonce, AccountId),
        VoteAgainst(ChainId, DepositNonce, AccountId),
        ProposalApproved(ChainId, DepositNonce),
        ProposalRejected(ChainId, DepositNonce),
        ProposalSucceeded(ChainId, DepositNonce),
        ProposalFailed(ChainId, DepositNonce),
    }
}

decl_error! {
    pub enum Error for Module<T: Trait> {
        ThresholdNotSet,
        InvalidChainId,
        InvalidThreshold,
        ChainNotWhitelisted,
        ChainAlreadyWhitelisted,
        ResourceDoesNotExist,
        RelayerAlreadyExists,
        RelayerInvalid,
        MustBeRelayer,
        RelayerAlreadyVoted,
        ProposalAlreadyExists,
        ProposalDoesNotExist,
        ProposalNotComplete,
        ProposalAlreadyComplete,
        ProposalExpired,
    }
}

decl_storage! {
    trait Store for Module<T: Trait> as ChainBridge {
        ChainNonces get(fn chains): map hasher(opaque_blake2_256) ChainId => Option<DepositNonce>;

        RelayerThreshold get(fn relayer_threshold): u32 = DEFAULT_RELAYER_THRESHOLD;

        pub Relayers get(fn relayers): map hasher(opaque_blake2_256) T::AccountId => bool;

        pub RelayerCount get(fn relayer_count): u32;

        pub Votes get(fn votes):
            double_map hasher(opaque_blake2_256) ChainId, hasher(opaque_blake2_256) (DepositNonce, T::Proposal)
            => Option<ProposalVotes<T::AccountId, T::BlockNumber>>;

        pub Resources get(fn resources):
            map hasher(opaque_blake2_256) ResourceId => Option<Vec<u8>>
    }
}

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        type Error = Error<T>;

        const ChainIdentity: ChainId = T::ChainId::get();
        const ProposalLifetime: T::BlockNumber = T::ProposalLifetime::get();
        const BridgeAccountId: T::AccountId = MODULE_ID.into_account();

        fn deposit_event() = default;

        #[weight = 195_000_000]
        pub fn set_threshold(origin, threshold: u32) -> DispatchResult {
            Self::ensure_admin(origin)?;
            Self::set_relayer_threshold(threshold)
        }

        #[weight = 195_000_000]
        pub fn set_resource(origin, id: ResourceId, method: Vec<u8>) -> DispatchResult {
            Self::ensure_admin(origin)?;
            Self::register_resource(id, method)
        }

        #[weight = 195_000_000]
        pub fn remove_resource(origin, id: ResourceId) -> DispatchResult {
            Self::ensure_admin(origin)?;
            Self::unregister_resource(id)
        }

        #[weight = 195_000_000]
        pub fn whitelist_chain(origin, id: ChainId) -> DispatchResult {
            Self::ensure_admin(origin)?;
            Self::whitelist(id)
        }

        #[weight = 195_000_000]
        pub fn add_relayer(origin, v: T::AccountId) -> DispatchResult {
            Self::ensure_admin(origin)?;
            Self::register_relayer(v)
        }

        #[weight = 195_000_000]
        pub fn remove_relayer(origin, v: T::AccountId) -> DispatchResult {
            Self::ensure_admin(origin)?;
            Self::unregister_relayer(v)
        }

        #[weight = (call.get_dispatch_info().weight + 195_000_000, call.get_dispatch_info().class, Pays::Yes)]
        pub fn acknowledge_proposal(origin, nonce: DepositNonce, src_id: ChainId, r_id: ResourceId, call: Box<<T as Trait>::Proposal>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(Self::is_relayer(&who), Error::<T>::MustBeRelayer);
            ensure!(Self::chain_whitelisted(src_id), Error::<T>::ChainNotWhitelisted);
            ensure!(Self::resource_exists(r_id), Error::<T>::ResourceDoesNotExist);

            Self::vote_for(who, nonce, src_id, call)
        }

        #[weight = 195_000_000]
        pub fn reject_proposal(origin, nonce: DepositNonce, src_id: ChainId, r_id: ResourceId, call: Box<<T as Trait>::Proposal>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(Self::is_relayer(&who), Error::<T>::MustBeRelayer);
            ensure!(Self::chain_whitelisted(src_id), Error::<T>::ChainNotWhitelisted);
            ensure!(Self::resource_exists(r_id), Error::<T>::ResourceDoesNotExist);

            Self::vote_against(who, nonce, src_id, call)
        }

        #[weight = (prop.get_dispatch_info().weight + 195_000_000, prop.get_dispatch_info().class, Pays::Yes)]
        pub fn eval_vote_state(origin, nonce: DepositNonce, src_id: ChainId, prop: Box<<T as Trait>::Proposal>) -> DispatchResult {
            ensure_signed(origin)?;

            Self::try_resolve_proposal(nonce, src_id, prop)
        }
    }
}

impl<T: Trait> Module<T> {

    pub fn ensure_admin(o: T::Origin) -> DispatchResult {
        T::AdminOrigin::try_origin(o)
            .map(|_| ())
            .or_else(ensure_root)?;
        Ok(())
    }

    pub fn is_relayer(who: &T::AccountId) -> bool {
        Self::relayers(who)
    }

    pub fn account_id() -> T::AccountId {
        MODULE_ID.into_account()
    }

    pub fn resource_exists(id: ResourceId) -> bool {
        return Self::resources(id) != None;
    }

    pub fn chain_whitelisted(id: ChainId) -> bool {
        return Self::chains(id) != None;
    }

    fn bump_nonce(id: ChainId) -> DepositNonce {
        let nonce = Self::chains(id).unwrap_or_default() + 1;
        <ChainNonces>::insert(id, nonce);
        nonce
    }

    pub fn set_relayer_threshold(threshold: u32) -> DispatchResult {
        ensure!(threshold > 0, Error::<T>::InvalidThreshold);
        <RelayerThreshold>::put(threshold);
        Self::deposit_event(RawEvent::RelayerThresholdChanged(threshold));
        Ok(())
    }

    pub fn register_resource(id: ResourceId, method: Vec<u8>) -> DispatchResult {
        <Resources>::insert(id, method);
        Ok(())
    }

    pub fn unregister_resource(id: ResourceId) -> DispatchResult {
        <Resources>::remove(id);
        Ok(())
    }

    pub fn whitelist(id: ChainId) -> DispatchResult {
        ensure!(id != T::ChainId::get(), Error::<T>::InvalidChainId);
        ensure!(
            !Self::chain_whitelisted(id),
            Error::<T>::ChainAlreadyWhitelisted
        );
        <ChainNonces>::insert(&id, 0);
        Self::deposit_event(RawEvent::ChainWhitelisted(id));
        Ok(())
    }

    pub fn register_relayer(relayer: T::AccountId) -> DispatchResult {
        ensure!(
            !Self::is_relayer(&relayer),
            Error::<T>::RelayerAlreadyExists
        );
        <Relayers<T>>::insert(&relayer, true);
        <RelayerCount>::mutate(|i| *i += 1);

        Self::deposit_event(RawEvent::RelayerAdded(relayer));
        Ok(())
    }

    pub fn unregister_relayer(relayer: T::AccountId) -> DispatchResult {
        ensure!(Self::is_relayer(&relayer), Error::<T>::RelayerInvalid);
        <Relayers<T>>::remove(&relayer);
        <RelayerCount>::mutate(|i| *i -= 1);
        Self::deposit_event(RawEvent::RelayerRemoved(relayer));
        Ok(())
    }

    fn commit_vote(
        who: T::AccountId,
        nonce: DepositNonce,
        src_id: ChainId,
        prop: Box<T::Proposal>,
        in_favour: bool,
    ) -> DispatchResult {
        let now = <frame_system::Module<T>>::block_number();
        let mut votes = match <Votes<T>>::get(src_id, (nonce, prop.clone())) {
            Some(v) => v,
            None => {
                let mut v = ProposalVotes::default();
                v.expiry = now + T::ProposalLifetime::get();
                v
            }
        };

        ensure!(!votes.is_complete(), Error::<T>::ProposalAlreadyComplete);
        ensure!(!votes.is_expired(now), Error::<T>::ProposalExpired);
        ensure!(!votes.has_voted(&who), Error::<T>::RelayerAlreadyVoted);

        if in_favour {
            votes.votes_for.push(who.clone());
            Self::deposit_event(RawEvent::VoteFor(src_id, nonce, who.clone()));
        } else {
            votes.votes_against.push(who.clone());
            Self::deposit_event(RawEvent::VoteAgainst(src_id, nonce, who.clone()));
        }

        <Votes<T>>::insert(src_id, (nonce, prop.clone()), votes.clone());

        Ok(())
    }

    fn try_resolve_proposal(
        nonce: DepositNonce,
        src_id: ChainId,
        prop: Box<T::Proposal>,
    ) -> DispatchResult {
        if let Some(mut votes) = <Votes<T>>::get(src_id, (nonce, prop.clone())) {
            let now = <frame_system::Module<T>>::block_number();
            ensure!(!votes.is_complete(), Error::<T>::ProposalAlreadyComplete);
            ensure!(!votes.is_expired(now), Error::<T>::ProposalExpired);

            let status = votes.try_to_complete(<RelayerThreshold>::get(), <RelayerCount>::get());
            <Votes<T>>::insert(src_id, (nonce, prop.clone()), votes.clone());

            match status {
                ProposalStatus::Approved => Self::finalize_execution(src_id, nonce, prop),
                ProposalStatus::Rejected => Self::cancel_execution(src_id, nonce),
                _ => Ok(()),
            }
        } else {
            Err(Error::<T>::ProposalDoesNotExist)?
        }
    }

    fn vote_for(
        who: T::AccountId,
        nonce: DepositNonce,
        src_id: ChainId,
        prop: Box<T::Proposal>,
    ) -> DispatchResult {
        Self::commit_vote(who, nonce, src_id, prop.clone(), true)?;
        Self::try_resolve_proposal(nonce, src_id, prop)
    }

    fn vote_against(
        who: T::AccountId,
        nonce: DepositNonce,
        src_id: ChainId,
        prop: Box<T::Proposal>,
    ) -> DispatchResult {
        Self::commit_vote(who, nonce, src_id, prop.clone(), false)?;
        Self::try_resolve_proposal(nonce, src_id, prop)
    }

    fn finalize_execution(
        src_id: ChainId,
        nonce: DepositNonce,
        call: Box<T::Proposal>,
    ) -> DispatchResult {
        Self::deposit_event(RawEvent::ProposalApproved(src_id, nonce));
        call.dispatch(frame_system::RawOrigin::Signed(Self::account_id()).into())
            .map(|_| ())
            .map_err(|e| e.error)?;
        Self::deposit_event(RawEvent::ProposalSucceeded(src_id, nonce));
        Ok(())
    }

    fn cancel_execution(src_id: ChainId, nonce: DepositNonce) -> DispatchResult {
        Self::deposit_event(RawEvent::ProposalRejected(src_id, nonce));
        Ok(())
    }

    pub fn transfer_fungible(
        dest_id: ChainId,
        resource_id: ResourceId,
        to: Vec<u8>,
        amount: U256,
    ) -> DispatchResult {
        ensure!(
            Self::chain_whitelisted(dest_id),
            Error::<T>::ChainNotWhitelisted
        );
        let nonce = Self::bump_nonce(dest_id);
        Self::deposit_event(RawEvent::FungibleTransfer(
            dest_id,
            nonce,
            resource_id,
            amount,
            to,
        ));
        Ok(())
    }

    pub fn transfer_nonfungible(
        dest_id: ChainId,
        resource_id: ResourceId,
        token_id: Vec<u8>,
        to: Vec<u8>,
        metadata: Vec<u8>,
    ) -> DispatchResult {
        ensure!(
            Self::chain_whitelisted(dest_id),
            Error::<T>::ChainNotWhitelisted
        );
        let nonce = Self::bump_nonce(dest_id);
        Self::deposit_event(RawEvent::NonFungibleTransfer(
            dest_id,
            nonce,
            resource_id,
            token_id,
            to,
            metadata,
        ));
        Ok(())
    }

    pub fn transfer_generic(
        dest_id: ChainId,
        resource_id: ResourceId,
        metadata: Vec<u8>,
    ) -> DispatchResult {
        ensure!(
            Self::chain_whitelisted(dest_id),
            Error::<T>::ChainNotWhitelisted
        );
        let nonce = Self::bump_nonce(dest_id);
        Self::deposit_event(RawEvent::GenericTransfer(
            dest_id,
            nonce,
            resource_id,
            metadata,
        ));
        Ok(())
    }
}

pub struct EnsureBridge<T>(sp_std::marker::PhantomData<T>);
impl<T: Trait> EnsureOrigin<T::Origin> for EnsureBridge<T> {
    type Success = T::AccountId;
    fn try_origin(o: T::Origin) -> Result<Self::Success, T::Origin> {
        let bridge_id = MODULE_ID.into_account();
        o.into().and_then(|o| match o {
            system::RawOrigin::Signed(who) if who == bridge_id => Ok(bridge_id),
            r => Err(T::Origin::from(r)),
        })
    }
}
