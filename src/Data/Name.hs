{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE EmptyDataDecls #-}
{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE MagicHash #-}
{-# LANGUAGE UnboxedTuples #-}

module Data.Name where

import Data.Utf8 qualified as Utf8

type Name =
    Utf8.Utf8 ELM_NAME

data ELM_NAME
