module Nash.Terminal.Check where

import Nash.Terminal.Common
  ( TraceLevel (..),
    Tracing (..),
    traceFilterReader,
    traceLevelReader,
  )
import Options.Applicative

data CheckArgs = CheckArgs
  { checkDirectory :: Maybe FilePath,
    checkDeny :: Bool,
    checkSilent :: Bool,
    checkSkipTests :: Bool,
    checkDebug :: Bool,
    checkShowJsonSchema :: Bool,
    checkWatch :: Bool,
    checkSeed :: Maybe Int,
    checkMaxSuccess :: Int,
    checkMatchTests :: Maybe [String],
    checkExactMatch :: Bool,
    checkEnv :: Maybe String,
    checkTraceFilter :: Maybe Tracing,
    checkTraceLevel :: TraceLevel
  }
  deriving (Show)

checkArgsParser :: Parser CheckArgs
checkArgsParser =
  CheckArgs
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
      ( short 's'
          <> long "skip-tests"
          <> help "Skip tests; run only the type-checker"
      )
    <*> switch
      ( long "debug"
          <> help "Also pretty-print test UPLC on failure"
      )
    <*> switch
      ( long "show-json-schema"
          <> help "Print JSON-schema of the command output when not a TTY"
      )
    <*> switch
      ( long "watch"
          <> help "Re-run the command on file changes instead of exiting"
      )
    <*> optional
      ( option
          auto
          ( long "seed"
              <> metavar "UINT"
              <> help "Initial seed for property-tests"
          )
      )
    <*> option
      auto
      ( long "max-success"
          <> metavar "UINT"
          <> help "Max successful test runs for property-tests"
          <> value 100
      )
    <*> optional
      ( some
          ( option
              str
              ( short 'm'
                  <> long "match-tests"
                  <> help "Only run tests matching these strings"
              )
          )
      )
    <*> switch
      ( short 'e'
          <> long "exact-match"
          <> help "Force exact test name matches with --match-tests"
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
          <> value Verbose
      )
