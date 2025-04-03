module Nash.Reporting.Annotation where

import Prelude hiding (traverse)

import Control.Monad (liftM2)
import Data.Binary (Binary, get, put)
import Data.Word (Word16)

-- | Located
data Located a
    = At Region a -- PERF see if unpacking region is helpful
    deriving (Show)

-- | Position
data Position
    = Position
        {-# UNPACK #-} !Word16
        {-# UNPACK #-} !Word16
    deriving (Eq, Show)

-- | Region
data Region = Region Position Position
    deriving (Eq, Show)
