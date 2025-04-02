{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE MagicHash #-}
{-# LANGUAGE UnboxedTuples #-}

module Data.Utf8 where

import GHC.Exts (isTrue#)
import GHC.Prim

data Utf8 tipe
    = Utf8 ByteArray#

-- EQUAL

instance Eq (Utf8 t) where
    (==) (Utf8 ba1#) (Utf8 ba2#) =
        let
            !len1# = sizeofByteArray# ba1#
            !len2# = sizeofByteArray# ba2#
         in
            isTrue# (len1# ==# len2#)
                && isTrue# (0# ==# compareByteArrays# ba1# 0# ba2# 0# len1#)

-- COMPARE
--
-- TODO: is it fine to sort by length and only compare bytes on length ties?
--

instance Ord (Utf8 t) where
    compare (Utf8 ba1#) (Utf8 ba2#) =
        let
            !len1# = sizeofByteArray# ba1#
            !len2# = sizeofByteArray# ba2#
            !len# = if isTrue# (len1# <# len2#) then len1# else len2#
            !cmp# = compareByteArrays# ba1# 0# ba2# 0# len#
         in
            case () of
                _
                    | isTrue# (cmp# <# 0#) -> LT
                    | isTrue# (cmp# ># 0#) -> GT
                    | isTrue# (len1# <# len2#) -> LT
                    | isTrue# (len1# ># len2#) -> GT
                    | True -> EQ
