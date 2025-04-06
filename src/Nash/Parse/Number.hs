{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE UnboxedTuples #-}

module Nash.Parse.Number (
    Number (..),
    number,
    Outcome (..),
    chompInt,
    chompHex,
    precedence,
)
where

import Data.Word (Word8)
import Foreign.Ptr (Ptr, minusPtr, plusPtr)

import Nash.Ast.Utils.BinOp qualified as BinOp
import Nash.Parse.Primitives (Col, Parser, Row)
import Nash.Parse.Primitives qualified as P
import Nash.Parse.Variable qualified as Var
import Nash.Reporting.Error.Syntax qualified as E

-- HELPERS

isDirtyEnd :: Ptr Word8 -> Ptr Word8 -> Word8 -> Bool
isDirtyEnd pos end word =
    Var.getInnerWidthHelp pos end word > 0

{-# INLINE isDecimalDigit #-}
isDecimalDigit :: Word8 -> Bool
isDecimalDigit word =
    word <= 0x39 {-9-} && word >= 0x30 {-0-}

-- NUMBERS

data Number
    = Int Int

number :: (Row -> Col -> x) -> (E.Number -> Row -> Col -> x) -> Parser x Number
number toExpectation toError =
    P.Parser $ \(P.State src pos end indent row col) cok _ cerr eerr ->
        if pos >= end
            then
                eerr row col toExpectation
            else
                let !word = P.unsafeIndex pos
                 in if not (isDecimalDigit word)
                        then
                            eerr row col toExpectation
                        else
                            let
                                outcome =
                                    if word == 0x30 {-0-}
                                        then
                                            chompZero (plusPtr pos 1) end
                                        else
                                            chompInt (plusPtr pos 1) end (fromIntegral (word - 0x30 {-0-}))
                             in
                                case outcome of
                                    Err newPos problem ->
                                        let
                                            !newCol = col + fromIntegral (minusPtr newPos pos)
                                         in
                                            cerr row newCol (toError problem)
                                    OkInt newPos n ->
                                        let
                                            !newCol = col + fromIntegral (minusPtr newPos pos)
                                            !integer = Int n
                                            !newState = P.State src newPos end indent row newCol
                                         in
                                            cok integer newState

-- CHOMP OUTCOME

-- first Int is newPos
data Outcome
    = Err (Ptr Word8) E.Number
    | OkInt (Ptr Word8) Int

-- CHOMP INT

chompInt :: Ptr Word8 -> Ptr Word8 -> Int -> Outcome
chompInt !pos end !n =
    if pos >= end
        then
            OkInt pos n
        else
            let
                !word = P.unsafeIndex pos
             in
                if isDecimalDigit word
                    then
                        chompInt (plusPtr pos 1) end (10 * n + fromIntegral (word - 0x30 {-0-}))
                    else
                        if word == 0x2E {-.-}
                            then
                                -- chompFraction pos end n
                                error "fractions not supported, we should return a good error message"
                            else
                                if word == 0x65 {-e-} || word == 0x45 {-E-}
                                    then
                                        -- chompExponent (plusPtr pos 1) end
                                        error "exponents not supported, we should return a good error message"
                                    else
                                        if isDirtyEnd pos end word
                                            then
                                                Err pos E.NumberEnd
                                            else
                                                OkInt pos n

-- CHOMP ZERO

chompZero :: Ptr Word8 -> Ptr Word8 -> Outcome
chompZero pos end =
    if pos >= end
        then
            OkInt pos 0
        else
            let !word = P.unsafeIndex pos
             in if word == 0x78 {-x-}
                    then
                        chompHexInt (plusPtr pos 1) end
                    else
                        if word == 0x2E {-.-}
                            then
                                -- chompFraction pos end 0
                                error "fractions not supported, we should return a good error message"
                            else
                                if isDecimalDigit word
                                    then
                                        Err pos E.NumberNoLeadingZero
                                    else
                                        if isDirtyEnd pos end word
                                            then
                                                Err pos E.NumberEnd
                                            else
                                                OkInt pos 0

chompHexInt :: Ptr Word8 -> Ptr Word8 -> Outcome
chompHexInt pos end =
    let (# newPos, answer #) = chompHex pos end
     in if answer < 0
            then
                Err newPos E.NumberHexDigit
            else
                OkInt newPos answer

-- CHOMP HEX

-- Return -1 if it has NO digits
-- Return -2 if it has BAD digits

{-# INLINE chompHex #-}
chompHex :: Ptr Word8 -> Ptr Word8 -> (# Ptr Word8, Int #)
chompHex pos end =
    chompHexHelp pos end (-1) 0

chompHexHelp :: Ptr Word8 -> Ptr Word8 -> Int -> Int -> (# Ptr Word8, Int #)
chompHexHelp pos end answer accumulator =
    if pos >= end
        then
            (# pos, answer #)
        else
            let
                !newAnswer =
                    stepHex pos end (P.unsafeIndex pos) accumulator
             in
                if newAnswer < 0
                    then
                        (# pos, if newAnswer == -1 then answer else -2 #)
                    else
                        chompHexHelp (plusPtr pos 1) end newAnswer newAnswer

{-# INLINE stepHex #-}
stepHex :: Ptr Word8 -> Ptr Word8 -> Word8 -> Int -> Int
stepHex pos end word acc
    | 0x30 {-0-} <= word && word <= 0x39 {-9-} = 16 * acc + fromIntegral (word - 0x30 {-0-})
    | 0x61 {-a-} <= word && word <= 0x66 {-f-} = 16 * acc + 10 + fromIntegral (word - 0x61 {-a-})
    | 0x41 {-A-} <= word && word <= 0x46 {-F-} = 16 * acc + 10 + fromIntegral (word - 0x41 {-A-})
    | isDirtyEnd pos end word = -2
    | True = -1

-- PRECEDENCE

precedence :: (Row -> Col -> x) -> Parser x BinOp.Precedence
precedence toExpectation =
    P.Parser $ \(P.State src pos end indent row col) cok _ _ eerr ->
        if pos >= end
            then
                eerr row col toExpectation
            else
                let !word = P.unsafeIndex pos
                 in if isDecimalDigit word
                        then
                            cok
                                (BinOp.Precedence (fromIntegral (word - 0x30 {-0-})))
                                (P.State src (plusPtr pos 1) end indent row (col + 1))
                        else
                            eerr row col toExpectation
