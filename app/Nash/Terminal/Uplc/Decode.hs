module Nash.Terminal.Uplc.Decode where

import Nash.Terminal.Common (Format (..), formatReader)
import Options.Applicative

data UplcDecodeArgs = UplcDecodeArgs
  { uplcDecodeInput :: FilePath,
    uplcDecodeFrom :: Format,
    uplcDecodeCbor :: Bool,
    uplcDecodeHex :: Bool
  }
  deriving (Show)

uplcDecodeArgsParser :: Parser UplcDecodeArgs
uplcDecodeArgsParser =
  UplcDecodeArgs
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
    <*> switch
      ( short 'c'
          <> long "cbor"
          <> help "Input file contains CBOR encoded flat bytes"
      )
    <*> switch
      ( long "hex"
          <> help "Input file contents will be hex decoded"
      )
