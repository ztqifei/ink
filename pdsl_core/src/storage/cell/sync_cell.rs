// Copyright 2018-2019 Parity Technologies (UK) Ltd.
// This file is part of pDSL.
//
// pDSL is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// pDSL is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with pDSL.  If not, see <http://www.gnu.org/licenses/>.

use crate::{
	storage::{
		cell::TypedCell,
		Allocator,
		Flush,
	},
	memory::boxed::Box,
};

use core::{
	cell::RefCell,
	pin::Pin,
};

/// A synchronized cell.
///
/// Provides interpreted, read-optimized and inplace-mutable
/// access to the associated constract storage slot.
///
/// # Guarantees
///
/// - `Owned`, `Typed`, `Avoid Reads`, `Mutable`
///
/// Read more about kinds of guarantees and their effect [here](../index.html#guarantees).
#[derive(Debug)]
pub struct SyncCell<T> {
	/// The underlying typed cell.
	cell: TypedCell<T>,
	/// The cache for the synchronized value.
	cache: Cache<T>,
}

/// A synchronized cache entry.
#[derive(Debug)]
pub struct SyncCacheEntry<T> {
	/// If the entry needs to be written back upon a flush.
	///
	/// This is required as soon as there are potential writes to the
	/// value stored in the associated cell.
	dirty: bool,
	/// The value of the cell.
	///
	/// Being captured in a `Pin` allows to provide robust references to the outside.
	cell_val: Pin<Box<Option<T>>>,
}

impl<T> SyncCacheEntry<T>
where
	T: Unpin
{
	/// Updates the cached value.
	pub fn update(&mut self, new_val: Option<T>) {
		*self.cell_val = new_val;
	}
}

impl<T> SyncCacheEntry<T> {
	/// Initializes this synchronized cache entry with the given value.
	///
	/// # Note
	///
	/// The cache will _not_ be marked as dirty after this operation.
	pub fn new(val: Option<T>) -> Self {
		Self{
			dirty: false,
			cell_val: Box::pin(val),
		}
	}

	/// Returns `true` if this synchronized cache entry is dirty.
	pub fn is_dirty(&self) -> bool {
		self.dirty
	}

	/// Marks the cached value as dirty.
	pub fn mark_dirty(&mut self) {
		self.dirty = true;
	}

	/// Marks the cached value as clean.
	pub fn mark_clean(&mut self) {
		self.dirty = false;
	}

	/// Returns an immutable reference to the synchronized cached value.
	pub fn get(&self) -> Option<&T> {
		(&*self.cell_val).into()
	}
}

impl<T> SyncCacheEntry<T>
where
	T: Unpin
{
	/// Returns a mutable reference to the synchronized cached value.
	///
	/// This also marks the cache entry as being dirty since
	/// the callee could potentially mutate the value.
	pub fn get_mut(&mut self) -> Option<&mut T> {
		self.mark_dirty();
		self.cell_val.as_mut().get_mut().into()
	}
}

/// A cache entry storing the value if synchronized.
#[derive(Debug)]
pub enum CacheEntry<T> {
	/// The cache is desychronized with the contract storage.
	Desync,
	/// The cache is in sync with the contract storage.
	Sync(SyncCacheEntry<T>),
}

impl<T> Default for CacheEntry<T> {
	fn default() -> Self {
		CacheEntry::Desync
	}
}

impl<T> CacheEntry<T>
where
	T: Unpin
{
	/// Updates the cached value.
	pub fn update(&mut self, new_val: Option<T>) {
		match self {
			CacheEntry::Desync => {
				*self = CacheEntry::Sync(SyncCacheEntry::new(new_val))
			}
			CacheEntry::Sync(sync_entry) => {
				sync_entry.update(new_val)
			}
		}
	}
}

impl<T> CacheEntry<T> {
	/// Returns `true` if the cache is in sync.
	pub fn is_synced(&self) -> bool {
		match self {
			CacheEntry::Sync(_) => true,
			_ => false,
		}
	}

	/// Returns `true` if the cache is dirty.
	pub fn is_dirty(&self) -> bool {
		match self {
			CacheEntry::Desync => false,
			CacheEntry::Sync(sync_entry) => sync_entry.is_dirty(),
		}
	}

	/// Marks the cache as dirty.
	pub fn mark_dirty(&mut self) {
		match self {
			CacheEntry::Sync(sync_entry) => sync_entry.mark_dirty(),
			CacheEntry::Desync => (),
		}
	}

	/// Marks the cache as clean.
	pub fn mark_clean(&mut self) {
		match self {
			CacheEntry::Sync(sync_entry) => sync_entry.mark_clean(),
			CacheEntry::Desync => (),
		}
	}

	/// Returns an immutable reference to the internal cached entity if any.
	///
	/// # Panics
	///
	/// If the cache is in desync state and thus has no cached entity.
	pub fn get(&self) -> Option<&T> {
		match self {
			CacheEntry::Desync => {
				panic!(
					"[pdsl_core::sync_cell::CacheEntry::get] Error: \
					 tried to get the value from a desync cache"
				)
			}
			CacheEntry::Sync(sync_entry) => {
				sync_entry.get()
			}
		}
	}
}

impl<T> CacheEntry<T>
where
	T: Unpin
{
	/// Returns a mutable reference to the internal cached entity if any.
	///
	/// # Panics
	///
	/// If the cache is in desync state and thus has no cached entity.
	pub fn get_mut(&mut self) -> Option<&mut T> {
		match self {
			CacheEntry::Desync => {
				panic!(
					"[pdsl_core::sync_cell::CacheEntry::get_mut] Error: \
					 tried to get the value from a desync cache"
				)
			}
			CacheEntry::Sync(sync_entry) => {
				sync_entry.get_mut()
			}
		}
	}
}

/// A cache for synchronizing values between memory and storage.
#[derive(Debug)]
pub struct Cache<T> {
	/// The cached value.
	entry: RefCell<CacheEntry<T>>,
}

impl<T> Default for Cache<T> {
	fn default() -> Self {
		Self{ entry: Default::default() }
	}
}

impl<T> Cache<T>
where
	T: Unpin
{
	/// Updates the synchronized value.
	///
	/// # Note
	///
	/// - The cache will be in sync after this operation.
	/// - The cache will not be dirty after this operation.
	pub fn update(&self, new_val: Option<T>) {
		self.entry.borrow_mut().update(new_val)
	}
}

impl<T> Cache<T> {
	/// Returns `true` if the cache is in sync.
	pub fn is_synced(&self) -> bool {
		self.entry.borrow().is_synced()
	}

	/// Returns `true` if the cache is dirty.
	pub fn is_dirty(&self) -> bool {
		self.entry.borrow().is_dirty()
	}

	/// Marks the cache dirty.
	pub fn mark_dirty(&mut self) {
		self.entry.borrow_mut().mark_dirty()
	}

	/// Marks the cache clean.
	pub fn mark_clean(&mut self) {
		self.entry.borrow_mut().mark_clean()
	}

	/// Returns an immutable reference to the internal cache entry.
	///
	/// Used to returns references from the inside to the outside.
	fn get_entry(&self) -> &CacheEntry<T> {
		unsafe { &*self.entry.as_ptr() }
	}

	/// Returns an immutable reference to the internal cache entry.
	///
	/// Used to returns references from the inside to the outside.
	fn get_entry_mut(&mut self) -> &mut CacheEntry<T> {
		unsafe { &mut *self.entry.as_ptr() }
	}

	/// Returns an immutable reference to the value if any.
	///
	/// # Panics
	///
	/// If the cache is desnyc and thus has no synchronized value.
	pub fn get(&self) -> Option<&T> {
		self.get_entry().get()
	}
}

impl<T> Cache<T>
where
	T: Unpin
{
	/// Returns an immutable reference to the value if any.
	///
	/// # Panics
	///
	/// If the cache is desnyc and thus has no synchronized value.
	pub fn get_mut(&mut self) -> Option<&mut T> {
		self.get_entry_mut().get_mut()
	}
}

impl<T> parity_codec::Encode for SyncCell<T> {
	fn encode_to<W: parity_codec::Output>(&self, dest: &mut W) {
		self.cell.encode_to(dest)
	}
}

impl<T> parity_codec::Decode for SyncCell<T> {
	fn decode<I: parity_codec::Input>(input: &mut I) -> Option<Self> {
		TypedCell::decode(input)
			.map(|typed_cell| Self{
				cell: typed_cell,
				cache: Cache::default()
			})
	}
}

impl<T> Flush for SyncCell<T>
where
	T: parity_codec::Encode,
{
	fn flush(&mut self) {
		if self.cache.is_dirty() {
			match self.cache.get() {
				Some(val) => self.cell.store(val),
				None => self.cell.clear(),
			}
			self.cache.mark_clean();
		}
	}
}

impl<T> SyncCell<T> {
	/// Allocates a new sync cell using the given storage allocator.
	///
	/// # Safety
	///
	/// The is unsafe because it does not check if the associated storage
	/// does not alias with storage allocated by other storage allocators.
	pub unsafe fn new_using_alloc<A>(alloc: &mut A) -> Self
	where
		A: Allocator
	{
		Self{
			cell: TypedCell::new_using_alloc(alloc),
			cache: Default::default(),
		}
	}
}

impl<T> SyncCell<T>
where
	T: Unpin
{
	/// Removes the value from the cell.
	pub fn clear(&mut self) {
		self.cache.update(None);
		self.cache.mark_dirty();
	}
}

impl<T> SyncCell<T>
where
	T: parity_codec::Decode + Unpin
{
	/// Returns an immutable reference to the value of the cell.
	pub fn get(&self) -> Option<&T> {
		if !self.cache.is_synced() {
			let loaded = self.cell.load();
			self.cache.update(loaded);
		}
		self.cache.get()
	}
}

impl<T> SyncCell<T>
where
	T: parity_codec::Encode + Unpin
{
	/// Sets the value of the cell.
	pub fn set(&mut self, val: T) {
		self.cache.update(Some(val));
		self.cache.mark_dirty();
	}
}

impl<T> SyncCell<T>
where
	T: parity_codec::Codec + Unpin,
{
	/// Returns a mutable reference to the value of the cell.
	pub fn get_mut(&mut self) -> Option<&mut T> {
		if !self.cache.is_synced() {
			let loaded = self.cell.load();
			self.cache.update(loaded);
		}
		self.cache.mark_dirty();
		self.cache.get_mut()
	}

	/// Mutates the value stored in the cell.
	///
	/// Returns an immutable reference to the result if
	/// a mutation happened, otherwise `None` is returned.
	pub fn mutate_with<F>(&mut self, f: F) -> Option<&T>
	where
		F: FnOnce(&mut T)
	{
		if let Some(value) = self.get_mut() {
			f(value);
			return Some(&*value)
		}
		None
	}
}

#[cfg(all(test, feature = "test-env"))]
mod tests {
	use super::*;

	use crate::{
		storage::{
			Key,
			alloc::BumpAlloc,
		},
		test_utils::run_test,
		env::TestEnv,
	};

	fn dummy_cell() -> SyncCell<i32> {
		unsafe {
			let mut alloc = BumpAlloc::from_raw_parts(
				Key([0x0; 32])
			);
			SyncCell::new_using_alloc(&mut alloc)
		}
	}

	#[test]
	fn simple() {
		run_test(|| {
			let mut cell = dummy_cell();
			assert_eq!(cell.get(), None);
			cell.set(5);
			assert_eq!(cell.get(), Some(&5));
			assert_eq!(cell.mutate_with(|val| *val += 10), Some(&15));
			assert_eq!(cell.get(), Some(&15));
			cell.clear();
			assert_eq!(cell.get(), None);
		})
	}

	#[test]
	fn count_rw_get() {
		// Repetitions performed.
		const N: u32 = 5;

		let mut cell = dummy_cell();

		// Asserts initial reads and writes are zero.
		assert_eq!(TestEnv::total_reads(), 0);
		assert_eq!(TestEnv::total_writes(), 0);

		// Repeated reads on the same cell.
		for _i in 0..N {
			cell.get();
			assert_eq!(TestEnv::total_reads(), 1);
			assert_eq!(TestEnv::total_writes(), 0);
		}

		// Flush the cell and assert reads and writes.
		cell.flush();
		assert_eq!(TestEnv::total_reads(), 1);
		assert_eq!(TestEnv::total_writes(), 0);
	}

	#[test]
	fn count_rw_get_mut() {
		// Repetitions performed.
		const N: u32 = 5;

		let mut cell = dummy_cell();

		// Asserts initial reads and writes are zero.
		assert_eq!(TestEnv::total_reads(), 0);
		assert_eq!(TestEnv::total_writes(), 0);

		// Repeated mutable reads on the same cell.
		for _i in 0..N {
			cell.get_mut();
			assert_eq!(TestEnv::total_reads(), 1);
			assert_eq!(TestEnv::total_writes(), 0);
		}

		// Flush the cell and assert reads and writes.
		cell.flush();
		assert_eq!(TestEnv::total_reads(), 1);
		assert_eq!(TestEnv::total_writes(), 1);
	}

	#[test]
	fn count_rw_set() {
		// Repetitions performed.
		const N: u32 = 5;

		let mut cell = dummy_cell();

		// Asserts initial reads and writes are zero.
		assert_eq!(TestEnv::total_reads(), 0);
		assert_eq!(TestEnv::total_writes(), 0);

		// Repeated writes to the same cell.
		for _i in 0..N {
			cell.set(42);
			assert_eq!(TestEnv::total_reads(), 0);
			assert_eq!(TestEnv::total_writes(), 0);
		}

		// Flush the cell and assert reads and writes.
		cell.flush();
		assert_eq!(TestEnv::total_reads(), 0);
		assert_eq!(TestEnv::total_writes(), 1);
	}

	#[test]
	fn count_rw_clear() {
		// Repetitions performed.
		const N: u32 = 5;

		let mut cell = dummy_cell();

		// Asserts initial reads and writes are zero.
		assert_eq!(TestEnv::total_reads(), 0);
		assert_eq!(TestEnv::total_writes(), 0);

		// Repeated writes to the same cell.
		for _i in 0..N {
			cell.clear();
			assert_eq!(TestEnv::total_reads(), 0);
			assert_eq!(TestEnv::total_writes(), 0);
		}

		// Flush the cell and assert reads and writes.
		cell.flush();
		assert_eq!(TestEnv::total_reads(), 0);
		assert_eq!(TestEnv::total_writes(), 1);
	}
}
