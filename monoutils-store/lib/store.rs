use std::{collections::HashSet, future::Future, pin::Pin};

use bytes::Bytes;
use ipld_core::cid::Cid;
use monoutils::SeekableReader;
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::{IpldReferences, StoreError, StoreResult};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// The different codecs supported by the IPLD store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Codec {
    /// Raw bytes.
    Raw,

    /// DAG-CBOR codec.
    DagCbor,

    /// DAG-JSON codec.
    DagJson,

    /// DAG-PB codec.
    DagPb,
}

//--------------------------------------------------------------------------------------------------
// Traits: IpldStore, IpldStoreSeekable, IpldStoreExt, *
//--------------------------------------------------------------------------------------------------

/// `IpldStore` is a content-addressable store for [`IPLD` (InterPlanetary Linked Data)][ipld] that
/// emphasizes the structured nature of the data it stores.
///
/// It can store raw bytes of data and structured data stored as IPLD. Stored data can be fetched
/// by their [`CID`s (Content Identifier)][cid] which is represents the fingerprint of the data.
///
/// ## Implementation Note
///
/// It is advisable that the type implementing `IpldStore` implements cheap clone semantics (e.g.,
/// using `Arc`) since several operations on `IpldStore` require cloning the store. Using types with
/// expensive clone operations may impact performance.
///
/// [cid]: https://docs.ipfs.tech/concepts/content-addressing/
/// [ipld]: https://ipld.io/
///
// TODO: Add support for deleting blocks with `derefence` method.
// TODO: Add support for specifying hash type.
pub trait IpldStore: RawStore + Clone {
    /// Saves an IPLD serializable object to the store and returns the `Cid` to it.
    ///
    /// ## Errors
    ///
    /// If the serialized data is too large, `StoreError::NodeBlockTooLarge` error is returned.
    fn put_node<T>(&self, data: &T) -> impl Future<Output = StoreResult<Cid>> + Send
    where
        T: Serialize + IpldReferences + Sync;

    /// Takes a reader of raw bytes, saves it to the store and returns the `Cid` to it.
    ///
    /// This method allows the store to chunk large amounts of data into smaller blocks to fit the
    /// storage medium and it may also involve creation of merkle nodes to represent the chunks.
    ///
    /// ## Errors
    ///
    /// If the bytes are too large, `StoreError::RawBlockTooLarge` error is returned.
    fn put_bytes<'a>(
        &'a self,
        reader: impl AsyncRead + Send + Sync + 'a,
    ) -> impl Future<Output = StoreResult<Cid>> + 'a;

    /// Gets a type stored as an IPLD data from the store by its `Cid`.
    ///
    /// ## Errors
    ///
    /// If the block is not found, `StoreError::BlockNotFound` error is returned.
    fn get_node<D>(&self, cid: &Cid) -> impl Future<Output = StoreResult<D>> + Send
    where
        D: DeserializeOwned + Send;

    /// Gets a reader for the underlying bytes associated with the given `Cid`.
    ///
    /// ## Errors
    ///
    /// If the block is not found, `StoreError::BlockNotFound` error is returned.
    fn get_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> impl Future<Output = StoreResult<Pin<Box<dyn AsyncRead + Send + Sync + 'a>>>> + 'a;

    /// Gets the size of all the blocks associated with the given `Cid` in bytes.
    fn get_bytes_size(&self, cid: &Cid) -> impl Future<Output = StoreResult<u64>> + Send;

    /// Checks if the store has a block with the given `Cid`.
    fn has(&self, cid: &Cid) -> impl Future<Output = bool>;

    /// Returns the codecs supported by the store.
    fn get_supported_codecs(&self) -> HashSet<Codec>;

    /// Returns the allowed maximum block size for IPLD and merkle nodes.
    /// If there is no limit, `None` is returned.
    fn get_node_block_max_size(&self) -> Option<u64>;

    /// Checks if the store is empty.
    fn is_empty(&self) -> impl Future<Output = StoreResult<bool>> {
        async {
            let count = self.get_block_count().await?;
            Ok(count == 0)
        }
    }

    /// Returns the number of blocks in the store.
    fn get_block_count(&self) -> impl Future<Output = StoreResult<u64>>;

    // /// Dereferences a CID and deletes its blocks if it is not referenced by any other CID.
    // ///
    // /// This can lead to a cascade of deletions if the referenced blocks are also not referenced by
    // /// any other CID.
    // ///
    // /// Returns `true` if the CID was deleted, `false` otherwise.
    // fn dereference(&self, cid: &Cid) -> impl Future<Output = StoreResult<bool>>;

    // /// Attempts to remove unused blocks from the store.
    // ///
    // /// Returns `true` if any blocks were removed, `false` otherwise.
    // fn gc(&self) -> impl Future<Output = StoreResult<bool>>;
}

/// A trait for stores that support raw blocks.
pub trait RawStore: Clone {
    /// Tries to save `bytes` as a single block to the store. Unlike `put_bytes`, this method does
    /// not chunk the data and does not create intermediate merkle nodes.
    ///
    /// ## Errors
    ///
    /// If the bytes are too large, `StoreError::RawBlockTooLarge` error is returned.
    fn put_raw_block(
        &self,
        bytes: impl Into<Bytes> + Send,
    ) -> impl Future<Output = StoreResult<Cid>> + Send;

    /// Retrieves raw bytes of a single block from the store by its `Cid`.
    ///
    /// Unlike `get_stream`, this method does not expect chunked data and does not have to retrieve
    /// intermediate merkle nodes.
    ///
    /// ## Errors
    ///
    /// If the block is not found, `StoreError::BlockNotFound` error is returned.
    fn get_raw_block(&self, cid: &Cid) -> impl Future<Output = StoreResult<Bytes>> + Send + Sync;

    /// Returns the allowed maximum block size for raw bytes. If there is no limit, `None` is returned.
    fn get_raw_block_max_size(&self) -> Option<u64>;
}

/// Helper extension to the `IpldStore` trait.
pub trait IpldStoreExt: IpldStore {
    /// Reads all the bytes associated with the given CID into a single [`Bytes`] type.
    fn read_all(&self, cid: &Cid) -> impl Future<Output = StoreResult<Bytes>> {
        async {
            let mut reader = self.get_bytes(cid).await?;
            let mut bytes = Vec::new();

            reader
                .read_to_end(&mut bytes)
                .await
                .map_err(StoreError::custom)?;

            Ok(Bytes::from(bytes))
        }
    }
}

/// `IpldStoreSeekable` is a trait that extends the `IpldStore` trait to allow for seeking.
pub trait IpldStoreSeekable: IpldStore {
    /// Gets a seekable reader for the underlying bytes associated with the given CID.
    fn get_seekable_bytes<'a>(
        &'a self,
        cid: &'a Cid,
    ) -> impl Future<Output = StoreResult<Pin<Box<dyn SeekableReader + Send + Sync + 'a>>>> + Send;
}

/// A trait for types that can be changed to a different store.
pub trait StoreSwitchable {
    /// The type of the entity.
    type WithStore<U: IpldStore>;

    /// Change the store used to persist the entity.
    fn change_store<U: IpldStore>(self, new_store: U) -> Self::WithStore<U>;
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl TryFrom<u64> for Codec {
    type Error = StoreError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0x55 => Ok(Codec::Raw),
            0x71 => Ok(Codec::DagCbor),
            0x0129 => Ok(Codec::DagJson),
            0x70 => Ok(Codec::DagPb),
            _ => Err(StoreError::UnsupportedCodec(value)),
        }
    }
}

impl From<Codec> for u64 {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::Raw => 0x55,
            Codec::DagCbor => 0x71,
            Codec::DagJson => 0x0129,
            Codec::DagPb => 0x70,
        }
    }
}

impl<T> IpldStoreExt for T where T: IpldStore {}
