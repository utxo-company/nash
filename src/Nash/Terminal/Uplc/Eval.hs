module Nash.Terminal.Uplc.Eval where

import Options.Applicative

data UplcEvalArgs = UplcEvalArgs
  { uplcEvalScript :: FilePath,
    uplcEvalFlat :: Bool,
    uplcEvalCbor :: Bool,
    uplcEvalDebug :: Bool,
    uplcEvalArgs :: [String]
  }
  deriving (Show)

uplcEvalArgsParser :: Parser UplcEvalArgs
uplcEvalArgsParser =
  UplcEvalArgs
    <$> argument
      str
      ( metavar "SCRIPT"
          <> help "UPLC script file"
      )
    <*> switch
      ( short 'f'
          <> long "flat"
          <> help "Use flat format"
      )
    <*> switch
      ( short 'c'
          <> long "cbor"
          <> help "Use CBOR format"
      )
    <*> switch
      ( short 'd'
          <> long "debug"
          <> help "Enable debug output"
      )
    <*> many
      ( argument
          str
          ( metavar "ARGS"
              <> help "Arguments to pass to the UPLC program"
          )
      )
