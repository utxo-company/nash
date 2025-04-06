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
    deriving (Show)

{-# INLINE getPosition #-}
getPosition :: Parser x A.Position
getPosition =
    Parser $ \state@(State _ _ _ _ row col) _ eok _ _ ->
        eok (A.Position row col) state

-- ONE OF WITH FALLBACK

{-# INLINE oneOfWithFallback #-}
oneOfWithFallback :: [Parser x a] -> a -> Parser x a -- PERF is this function okay? Worried about allocation/laziness with fallback values.
oneOfWithFallback parsers fallback =
    Parser $ \state cok eok cerr _ ->
        oowfHelp state cok eok cerr parsers fallback

oowfHelp ::
    State ->
    (a -> State -> b) ->
    (a -> State -> b) ->
    (Row -> Col -> (Row -> Col -> x) -> b) ->
    [Parser x a] ->
    a ->
    b
oowfHelp state cok eok cerr parsers fallback =
    case parsers of
        [] ->
            eok fallback state
        Parser parser : parsers' ->
            let
                eerr' _ _ _ =
                    oowfHelp state cok eok cerr parsers' fallback
             in
                parser state cok eok cerr eerr'

-- FROM BYTESTRING
fromByteString :: Parser x a -> (Row -> Col -> x) -> B.ByteString -> Either x a
fromByteString (Parser parser) toBadEnd (B.PS fptr offset length) =
    B.accursedUnutterablePerformIO $
        let
            toOk' = toOk toBadEnd
            !pos = plusPtr (unsafeForeignPtrToPtr fptr) offset
            !end = plusPtr pos length
            !result = parser (State fptr pos end 0 1 1) toOk' toOk' toErr toErr
         in
            do
                touchForeignPtr fptr
                return result

toOk :: (Row -> Col -> x) -> a -> State -> Either x a
toOk toBadEnd !a (State _ pos end _ row col) =
    if pos == end
        then Right a
        else Left (toBadEnd row col)

toErr :: Row -> Col -> (Row -> Col -> x) -> Either x a
toErr row col toError =
    Left (toError row col)

-- LOW-LEVEL CHECKS

unsafeIndex :: Ptr Word8 -> Word8
unsafeIndex ptr =
    B.accursedUnutterablePerformIO (peek ptr)

{-# INLINE isWord #-}
isWord :: Ptr Word8 -> Ptr Word8 -> Word8 -> Bool
isWord pos end word =
    pos < end && unsafeIndex pos == word

getCharWidth :: Word8 -> Int
getCharWidth word
    | word < 0x80 = 1
    | word < 0xc0 = error "Need UTF-8 encoded input. Ran into unrecognized bits."
    | word < 0xe0 = 2
    | word < 0xf0 = 3
    | word < 0xf8 = 4
    | True = error "Need UTF-8 encoded input. Ran into unrecognized bits."
