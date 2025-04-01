module Nash.Terminal.Uplc.Shrink where

import Nash.Terminal.Common (Format (..), formatReader)
import Options.Applicative

data UplcShrinkArgs = UplcShrinkArgs
    { uplcShrinkInput :: FilePath
    , uplcShrinkFrom :: Format
    , uplcShrinkTo :: Format
    , uplcShrinkCbor :: Bool
    , uplcShrinkHex :: Bool
    }
    deriving (Show)

uplcShrinkArgsParser :: Parser UplcShrinkArgs
uplcShrinkArgsParser =
    UplcShrinkArgs
        <$> argument
            str
            ( metavar "INPUT"
                <> help "Flat encoded Untyped Plutus Core file"
            )
        <*> option
            formatReader
            ( long "from"
                <> help "Format to convert from"
                <> value DeBruijn
            )
        <*> option
            formatReader
            ( long "to"
                <> help "Format to convert into"
                <> value DeBruijn
            )
        <*> switch
            ( short 'c'
                <> long "cbor"
                <> help "Input file contains CBOR encoded flat bytes"
            )
        <*> switch
            ( long "hex"
                <> help "Input file contents will be hex decoded"
            )
