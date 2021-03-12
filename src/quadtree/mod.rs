//! Trees which allow nodes to have either zero children or exactly **4**, most often used to partition a 2D space by recursively subdividing it into four quadrants.
//!
//! The [Wikipedia article] on quadtrees covers their use cases and specifics in more detail.
//!
//! # Example
//! ```rust
//! use charcoal::quadtree::{Quadtree, NodeRef};
//!
//! // Create the tree. The only thing we need for that is the data payload for the root node. The
//! // turbofish there is needed to state that we are using the default storage method instead of
//! // asking the compiler to infer it, which would be impossible.
//! let mut tree = Quadtree::<_>::new(451);
//!
//! // Let's now try to access the structure of the tree and look around.
//! let root = tree.root();
//! // We have never added any nodes to the tree, so the root does not have any children, hence:
//! assert!(root.is_leaf());
//!
//! // Let's replace our reference to the root with a mutable one, to mutate the tree!
//! let mut root = tree.root_mut();
//! // First things first, we want to change our root's data payload:
//! *(root.value_mut().into_inner()) = 120;
//! // While we're at it, let's add some child nodes:
//! let my_numbers = [
//!     // Random numbers are not what you'd typically see in a quadtree, but for the sake of this
//!     // example we can use absolutely any kind of data. Bonus points for finding hidden meaning.
//!     2010, 2014, 1987, 1983,
//! ];
//! root.make_branch(my_numbers).unwrap();
//!
//! // Let's return to an immutable reference and look at our tree.
//! let root = NodeRef::from(root); // Conversion from a mutable to an immutable reference
//! assert_eq!(root.value().into_inner(), &120);
//! let children = {
//!     let children_refs = root.children().unwrap();
//!     let get_val = |x| {
//!         // Type inference decided to abandon us here
//!         let x: NodeRef<'_, _, _, _> = children_refs[x];
//!         *x.value().into_inner()
//!     };
//!     [ get_val(0), get_val(1), get_val(2), get_val(3) ]
//! };
//! assert_eq!(children, my_numbers);
//! ```
//!
//! [Wikipedia article]: https://en.wikipedia.org/wiki/Quadtree " "

use core::{
    fmt::Debug,
    iter::{DoubleEndedIterator, ExactSizeIterator, FusedIterator},
    borrow::{Borrow, BorrowMut},
    hint,
};
use crate::{
    storage::{Storage, ListStorage, DefaultStorage, SparseStorage, SparseStorageSlot},
    traversal::{
        Traversable,
        TraversableMut,
        VisitorDirection,
        CursorResult,
        CursorDirectionError,
    },
    NodeValue,
    TryRemoveBranchError,
    TryRemoveLeafError,
    TryRemoveChildrenError,
    util::{ArrayMap, unreachable_debugchecked},
};
use arrayvec::{ArrayVec, IntoIter as ArrayVecIntoIter};

mod node;
mod node_ref;

use node::NodeData;
pub use node::Node;
pub use node_ref::{NodeRef, NodeRefMut};

/// A quadtree.
///
/// See the [module-level documentation] for more.
///
/// [module-level documentation]: index.html " "
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Quadtree<B, L = B, K = usize, S = DefaultStorage<Node<B, L, K>>>
where
    S: Storage<Element = Node<B, L, K>, Key = K>,
    K: Clone + Debug + Eq,
{
    storage: S,
    root: K,
}
impl<B, L, K, S> Quadtree<B, L, K, S>
where
    S: Storage<Element = Node<B, L, K>, Key = K>,
    K: Clone + Debug + Eq,
{
    /// Creates a quadtree with the specified value for the root node.
    ///
    /// # Example
    /// ```rust
    /// # use charcoal::Quadtree;
    /// // The only way to create a tree...
    /// let tree = Quadtree::<_>::new(87);
    /// // ...is to simply create the root leaf node and storage. The turbofish there is needed to
    /// // state that we are using the default storage method instead of asking the compiler to
    /// // infer it, which would be impossible.
    ///
    /// // No other nodes have been created yet:
    /// assert!(tree.root().is_leaf());
    /// ```
    pub fn new(root: L) -> Self {
        let mut storage = S::new();
        let root = storage.add(unsafe {
            // SAFETY: there isn't a root there yet
            Node::root(root)
        });
        Self { storage, root }
    }
    /// Creates a quadtree with the specified capacity for the storage.
    ///
    /// # Panics
    /// The storage may panic if it has fixed capacity and the specified value does not match it.
    ///
    /// # Example
    /// ```rust
    /// # use charcoal::Quadtree;
    /// // Let's create a tree, but with some preallocated space for more nodes:
    /// let mut tree = Quadtree::<_>::with_capacity(5, "Variable Names");
    /// // The turbofish there is needed to state that we are using the default storage method
    /// // instead of asking the compiler to infer it, which would be impossible.
    ///
    /// // Capacity does not affect the actual nodes:
    /// assert!(tree.root().is_leaf());
    ///
    /// // Not until we create them ourselves:
    /// tree.root_mut().make_branch([
    ///     "Foo", "Bar", "Baz", "Quux",
    /// ]);
    ///
    /// // If the default storage is backed by a dynamic memory allocation,
    /// // at most one has happened to this point.
    /// ```
    pub fn with_capacity(capacity: usize, root: L) -> Self {
        let mut storage = S::with_capacity(capacity);
        let root = storage.add(unsafe {
            // SAFETY: as above
            Node::root(root)
        });
        Self { storage, root }
    }

    /// Returns a reference to the root node of the tree.
    ///
    /// # Example
    /// ```rust
    /// # use charcoal::Quadtree;
    /// // A tree always has a root node:
    /// let tree = Quadtree::<_>::new("Root");
    ///
    /// assert_eq!(
    ///     // The into_inner() call extracts data from a NodeValue, which is used to generalize
    ///     // tres to both work with same and different types for payloads of leaf and branch
    ///     // nodes.
    ///     *tree.root().value().into_inner(),
    ///     "Root",
    /// );
    /// ```
    #[allow(clippy::missing_const_for_fn)] // there cannot be constant trees just yet
    pub fn root(&self) -> NodeRef<'_, B, L, K, S> {
        unsafe {
            // SAFETY: binary trees cannot be created without a root
            NodeRef::new_raw_unchecked(self, self.root.clone())
        }
    }
    /// Returns a *mutable* reference to the root node of the tree, allowing modifications to the entire tree.
    ///
    /// # Example
    /// ```rust
    /// # use charcoal::Quadtree;
    /// // A tree always has a root node:
    /// let mut tree = Quadtree::<_>::new("Root");
    ///
    /// let mut root_mut = tree.root_mut();
    /// // The into_inner() call extracts data from a NodeValue, which is used to generalize trees
    /// // to both work with same and different types for payloads of leaf and branch nodes.
    /// *(root_mut.value_mut().into_inner()) = "The Source of the Beer";
    /// ```
    pub fn root_mut(&mut self) -> NodeRefMut<'_, B, L, K, S> {
        unsafe {
            // SAFETY: as above
            NodeRefMut::new_raw_unchecked(self, self.root.clone())
        }
    }
}
impl<B, L, S> Quadtree<B, L, usize, SparseStorage<Node<B, L, usize>, S>>
where
    S: ListStorage<Element = SparseStorageSlot<Node<B, L, usize>>>,
{
    /// Removes all holes from the sparse storage.
    ///
    /// Under the hood, this uses `defragment_and_fix`. It's not possible to defragment without fixing the indicies, as that might cause undefined behavior.
    ///
    /// # Example
    /// ```rust
    /// use charcoal::quadtree::SparseVecQuadtree;
    ///
    /// // Create a tree which explicitly uses sparse storage:
    /// let mut tree = SparseVecQuadtree::new(0);
    /// // This is already the default, but for the sake of this example we'll stay explicit.
    ///
    /// // Add some elements for the holes to appear:
    /// tree.root_mut().make_branch([
    ///     1, 2, 3, 4,
    /// ]).unwrap(); // You can replace this with proper error handling
    /// tree
    ///     .root_mut()
    ///     .nth_child_mut(0)
    ///     .unwrap() // This too
    ///     .make_branch([5, 6, 7, 8])
    ///     .unwrap(); // And this
    ///
    /// tree
    ///     .root_mut()
    ///     .nth_child_mut(0)
    ///     .unwrap() // Same as above
    ///     .try_remove_children()
    ///     .unwrap(); // Same here
    ///
    /// // We ended up creating 4 holes:
    /// assert_eq!(tree.num_holes(), 4);
    /// // Let's patch them:
    /// tree.defragment();
    /// // Now there are none:
    /// assert_eq!(tree.num_holes(), 0);
    /// ```
    pub fn defragment(&mut self) {
        self.storage.defragment_and_fix()
    }
    /// Returns the number of holes in the storage. This operation returns immediately instead of looping through the entire storage, since the sparse storage automatically tracks the number of holes it creates and destroys.
    ///
    /// # Example
    /// See the example in [`defragment`].
    ///
    /// [`defragment`]: #method.defragment " "
    pub fn num_holes(&self) -> usize {
        self.storage.num_holes()
    }
    /// Returns `true` if there are no holes in the storage, `false` otherwise. This operation returns immediately instead of looping through the entire storage, since the sparse storage automatically tracks the number of holes it creates and destroys.
    ///
    /// # Example
    /// See the example in [`defragment`].
    ///
    /// [`defragment`]: #method.defragment " "
    pub fn is_dense(&self) -> bool {
        self.storage.is_dense()
    }
}

impl<B, L, K, S> Traversable for Quadtree<B, L, K, S>
where
    S: Storage<Element = Node<B, L, K>, Key = K>,
    K: Clone + Debug + Eq,
{
    type Leaf = L;
    type Branch = B;
    type Cursor = K;

    fn advance_cursor<V>(
        &self,
        cursor: Self::Cursor,
        direction: VisitorDirection<Self::Cursor, V>,
    ) -> CursorResult<Self::Cursor> {
        // Create the error in advance to avoid duplication
        let error = CursorDirectionError {
            previous_state: cursor.clone(),
        };
        let node = NodeRef::new_raw(self, cursor)
            .expect("the node specified by the cursor does not exist");
        match direction {
            VisitorDirection::Parent => node.parent().ok_or(error).map(NodeRef::into_raw_key),
            VisitorDirection::NextSibling => {
                node.child_index()
                    .map(|child_index| {
                        let parent = node.parent().unwrap_or_else(|| unsafe {
                            unreachable_debugchecked("parent nodes cannot be leaves")
                        });
                        parent
                            .nth_child(child_index)
                            .unwrap_or_else(|| unsafe {
                                // SAFETY: the previous unreachable_debugchecked checked for this
                                hint::unreachable_unchecked()
                            })
                            .into_raw_key()
                    })
                    .ok_or(error)
            }
            VisitorDirection::Child(num) => {
                let num = if num <= 3 {
                    num as u8
                } else {
                    return Err(error);
                };
                node.nth_child(num).map(NodeRef::into_raw_key).ok_or(error)
            }
            VisitorDirection::SetTo(new_cursor) => {
                if self.storage.contains_key(&new_cursor) {
                    Ok(new_cursor)
                } else {
                    // Do not allow returning invalid cursors, as those will cause panicking
                    Err(error)
                }
            }
            VisitorDirection::Stop(..) => Err(error),
        }
    }
    fn cursor_to_root(&self) -> Self::Cursor {
        self.root.clone()
    }
    #[track_caller]
    fn value_of(&self, cursor: &Self::Cursor) -> NodeValue<&'_ Self::Branch, &'_ Self::Leaf> {
        let node_ref = NodeRef::new_raw(self, cursor.clone())
            .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor));
        node_ref.value()
    }
    #[track_caller]
    fn parent_of(&self, cursor: &Self::Cursor) -> Option<Self::Cursor> {
        let node_ref = NodeRef::new_raw(self, cursor.clone())
            .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor));
        node_ref.parent().map(NodeRef::into_raw_key)
    }
    #[track_caller]
    fn num_children_of(&self, cursor: &Self::Cursor) -> usize {
        let node_ref = NodeRef::new_raw(self, cursor.clone())
            .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor));
        if node_ref.is_branch() {
            4
        } else {
            0
        }
    }
    #[track_caller]
    fn nth_child_of(&self, cursor: &Self::Cursor, child_num: usize) -> Option<Self::Cursor> {
        if child_num < 4 {
            let node_ref = NodeRef::new_raw(self, cursor.clone())
                .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor));
            node_ref
                .nth_child(child_num as u8)
                .map(NodeRef::into_raw_key)
        } else {
            None
        }
    }
}
impl<B, L, K, S> TraversableMut for Quadtree<B, L, K, S>
where
    S: Storage<Element = Node<B, L, K>, Key = K>,
    K: Clone + Debug + Eq,
{
    const CAN_REMOVE_INDIVIDUAL_CHILDREN: bool = false;
    const CAN_PACK_CHILDREN: bool = true;
    type PackedChildren = PackedChildren<L>;

    #[track_caller]
    fn value_mut_of(
        &mut self,
        cursor: &Self::Cursor,
    ) -> NodeValue<&'_ mut Self::Branch, &'_ mut Self::Leaf> {
        self.storage
            .get_mut(cursor)
            .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor))
            .value
            .as_mut()
            .into_value()
    }
    fn try_remove_leaf<BtL: FnOnce(Self::Branch) -> Self::Leaf>(
        &mut self,
        _cursor: &Self::Cursor,
        _branch_to_leaf: BtL,
    ) -> Result<Self::Leaf, TryRemoveLeafError> {
        Err(TryRemoveLeafError::CannotRemoveIndividualChildren)
    }
    fn try_remove_branch_into<BtL: FnOnce(Self::Branch) -> Self::Leaf, C: FnMut(Self::Leaf)>(
        &mut self,
        _cursor: &Self::Cursor,
        _branch_to_leaf: BtL,
        _collector: C,
    ) -> Result<Self::Branch, TryRemoveBranchError> {
        Err(TryRemoveBranchError::CannotRemoveIndividualChildren)
    }
    #[track_caller]
    fn try_remove_children_into<BtL: FnOnce(Self::Branch) -> Self::Leaf, C: FnMut(Self::Leaf)>(
        &mut self,
        cursor: &Self::Cursor,
        branch_to_leaf: BtL,
        mut collector: C,
    ) -> Result<(), TryRemoveChildrenError> {
        let mut node_ref = NodeRefMut::new_raw(self, cursor.clone())
            .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor));
        node_ref.try_remove_children_with(branch_to_leaf).map(|x| {
            x.array_map(|e| collector(e));
        })
    }
    fn try_remove_branch<BtL: FnOnce(Self::Branch) -> Self::Leaf>(
        &mut self,
        _cursor: &Self::Cursor,
        _branch_to_leaf: BtL,
    ) -> Result<(Self::Branch, Self::PackedChildren), TryRemoveBranchError> {
        Err(TryRemoveBranchError::CannotRemoveIndividualChildren)
    }
    #[track_caller]
    fn try_remove_children<BtL: FnOnce(Self::Branch) -> Self::Leaf>(
        &mut self,
        cursor: &Self::Cursor,
        branch_to_leaf: BtL,
    ) -> Result<Self::PackedChildren, TryRemoveChildrenError> {
        let mut node_ref = NodeRefMut::new_raw(self, cursor.clone())
            .unwrap_or_else(|| panic!("invalid cursor: {:?}", cursor));
        node_ref
            .try_remove_children_with(branch_to_leaf)
            .map(Into::into)
    }
}

/// Packed leaf children nodes of an quadtree's branch node.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PackedChildren<T>(pub [T; 4]);
impl<T> PackedChildren<T> {
    /// Returns the packed children as an array.
    #[allow(clippy::missing_const_for_fn)] // cannot drop at compile time smh
    pub fn into_inner(self) -> [T; 4] {
        self.0
    }
}
impl<T> Borrow<[T]> for PackedChildren<T> {
    fn borrow(&self) -> &[T] {
        &self.0
    }
}
impl<T> BorrowMut<[T]> for PackedChildren<T> {
    fn borrow_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}
impl<T> IntoIterator for PackedChildren<T> {
    type Item = T;
    type IntoIter = PackedChildrenIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.into()
    }
}
impl<T> From<[T; 4]> for PackedChildren<T> {
    fn from(op: [T; 4]) -> Self {
        Self(op)
    }
}

/// An owned iterator over the elements of `PackedChildren`.
#[derive(Clone, Debug)]
pub struct PackedChildrenIter<T>(ArrayVecIntoIter<[T; 4]>);
impl<T> From<PackedChildren<T>> for PackedChildrenIter<T> {
    fn from(op: PackedChildren<T>) -> Self {
        Self(ArrayVec::from(op.0).into_iter())
    }
}
impl<T> Iterator for PackedChildrenIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl<T> DoubleEndedIterator for PackedChildrenIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back()
    }
}
impl<T> ExactSizeIterator for PackedChildrenIter<T> {
    fn len(&self) -> usize {
        self.0.len()
    }
}
impl<T> FusedIterator for PackedChildrenIter<T> {}

/// A quadtree which uses a *sparse* `Vec` as backing storage.
///
/// The default `Quadtree` type already uses this, so this is only provided for explicitness and consistency.
#[cfg(feature = "alloc")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
#[allow(unused_qualifications)]
pub type SparseVecQuadtree<B, L = B> =
    Quadtree<B, L, usize, crate::storage::SparseVec<Node<B, L, usize>>>;
/// A quadtree which uses a `Vec` as backing storage.
///
/// The default `Quadtree` type uses `Vec` with sparse storage. Not using sparse storage is heavily discouraged, as the memory usage penalty is negligible. Still, this is provided for convenience.
#[cfg(feature = "alloc")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
#[allow(unused_qualifications)]
pub type VecQuadtree<B, L = B> = Quadtree<B, L, usize, alloc::vec::Vec<Node<B, L, usize>>>;
