{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE EmptyDataDecls #-}
{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE UnboxedTuples #-}

module Nash.Package where

import Data.Utf8 qualified as Utf8

-- PACKGE NAMES

data Name
    = Name
    { _author :: !Author
    , _project :: !Project
    }
    deriving (Ord)

type Author = Utf8.Utf8 AUTHOR
type Project = Utf8.Utf8 PROJECT

data AUTHOR
data PROJECT

-- INSTANCES

instance Eq Name where
    (==) (Name author1 project1) (Name author2 project2) =
        project1 == project2 && author1 == author2
