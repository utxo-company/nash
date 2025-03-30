module Nash.Terminal.Uplc.Encode where

import Nash.Terminal.Common (Format (..), formatReader)
import Options.Applicative

data UplcEncodeArgs = UplcEncodeArgs
  { uplcEncodeInput :: FilePath,
    uplcEncodeTo :: Format,
    uplcEncodeCbor :: Bool,
    uplcEncodeHex :: Bool
  }
  deriving (Show)

uplcEncodeArgsParser :: Parser UplcEncodeArgs
uplcEncodeArgsParser =
  UplcEncodeArgs
    <$> argument
      str
      ( metavar "INPUT"
          <> help "Textual Untyped Plutus Core file"
      )
    <*> option
      formatReader
      ( long "to"
          <> help "Format to convert to"
          <> value DeBruijn
      )
    <*> switch
      ( short 'c'
          <> long "cbor"
          <> help "Further encode flat bytes as CBOR"
      )
    <*> switch
      ( long "hex"
          <> help "Hex encode the bytes"
      )
