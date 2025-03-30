module Nash.Terminal.Build where

import Nash.Terminal.Common
  ( TraceLevel (..),
    Tracing (..),
    traceFilterReader,
    traceLevelReader,
  )
import Options.Applicative

data BuildArgs = BuildArgs
  { buildDirectory :: Maybe FilePath,
    buildDeny :: Bool,
    buildSilent :: Bool,
    buildWatch :: Bool,
    buildUplc :: Bool,
    buildEnv :: Maybe String,
    buildOutput :: Maybe FilePath,
    buildTraceFilter :: Maybe Tracing,
    buildTraceLevel :: TraceLevel
  }
  deriving (Show)

buildArgsParser :: Parser BuildArgs
buildArgsParser =
  BuildArgs
    <$> optional
      ( argument
          str
          ( metavar "DIRECTORY"
              <> help "Path to project"
          )
      )
    <*> switch
      ( short 'D'
          <> long "deny"
          <> help "Deny warnings; warnings will be treated as errors"
      )
    <*> switch
      ( short 'S'
          <> long "silent"
          <> help "Silence warnings; warnings will not be printed"
      )
    <*> switch
      ( short 'w'
          <> long "watch"
          <> help "Re-run the command on file changes instead of exiting"
      )
    <*> switch
      ( short 'u'
          <> long "uplc"
          <> help "Also dump textual uplc"
      )
    <*> optional
      ( option
          str
          ( long "env"
              <> help "Environment to build against"
          )
      )
    <*> optional
      ( option
          str
          ( short 'o'
              <> long "out"
              <> metavar "FILEPATH"
              <> help "Optional relative filepath to the generated Plutus blueprint [default: plutus.json]"
          )
      )
    <*> optional
      ( option
          traceFilterReader
          ( short 'f'
              <> long "filter-traces"
              <> help "Filter traces to include (user-defined, compiler-generated, all)"
              <> value All
          )
      )
    <*> option
      traceLevelReader
      ( short 't'
          <> long "trace-level"
          <> help "Choose the verbosity level of traces"
          <> value Silent
      )
