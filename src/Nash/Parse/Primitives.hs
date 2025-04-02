module Nash.Parse.Primitives where

import Prelude hiding (length)

import Control.Applicative qualified as Applicative (Applicative (..))
import Data.ByteString.Internal qualified as B
import Data.Word (Word16, Word8)
import Foreign.ForeignPtr (ForeignPtr, touchForeignPtr)
import Foreign.ForeignPtr.Unsafe (unsafeForeignPtrToPtr)
import Foreign.Ptr (Ptr, plusPtr)
import Foreign.Storable (peek)

import Nash.Reporting.Annotation qualified as A

-- | Parser
newtype Parser x a
    = Parser
        ( forall b.
          State ->
          (a -> State -> b) -> -- consumed ok
          (a -> State -> b) -> -- empty ok
          (Row -> Col -> (Row -> Col -> x) -> b) -> -- consumed err
          (Row -> Col -> (Row -> Col -> x) -> b) -> -- empty err
          b
        )

data State -- PERF try taking some out to avoid allocation
    = State
    { _src :: ForeignPtr Word8
    , _pos :: !(Ptr Word8)
    , _end :: !(Ptr Word8)
    , _indent :: !Word16
    , _row :: !Row
    , _col :: !Col
    }

type Row = Word16
type Col = Word16

-- FROM SNIPPET

data Snippet
    = Snippet
    { _fptr :: ForeignPtr Word8
    , _offset :: Int
    , _length :: Int
    , _offRow :: Row
    , _offCol :: Col
    }
