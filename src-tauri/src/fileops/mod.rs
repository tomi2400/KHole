//! Move / copy / trash with the non-negotiable safety rules:
//! never delete (Trash via `NSFileManager.trashItemAtURL:resultingItemURL:`), never overwrite
//! (`name (2).ext` on conflicts), log every action for undo, touch only source and destination
//! folders. Implemented in M4–M5.
