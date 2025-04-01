module Nash.Terminal.Uplc.Fmt where

import Options.Applicative

data UplcFmtArgs = UplcFmtArgs
    { uplcFmtInput :: FilePath
    , uplcFmtPrint :: Bool
    }
    deriving (Show)

uplcFmtArgsParser :: Parser UplcFmtArgs
uplcFmtArgsParser =
    UplcFmtArgs
        <$> argument str (metavar "INPUT" <> help "Textual Untyped Plutus Core file")
        <*> switch (short 'p' <> long "print" <> help "Print output instead of saving to file")
