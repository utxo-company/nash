module Nash.Terminal.Common where

import Options.Applicative (ReadM, eitherReader)

data TraceLevel = Silent | Compact | Verbose deriving (Show, Eq)

data Tracing = UserDefined | CompilerGenerated | All deriving (Show)

data Format = DeBruijn deriving (Show) -- Placeholder; extend as needed

-- Reader for TraceLevel
traceLevelReader :: ReadM TraceLevel
traceLevelReader = eitherReader $ \s -> case s of
  "silent" -> Right Silent
  "compact" -> Right Compact
  "verbose" -> Right Verbose
  _ -> Left "Invalid trace level; must be 'silent', 'compact', or 'verbose'"

-- Reader for TraceFilter (returns a function from TraceLevel to Tracing)
traceFilterReader :: ReadM Tracing
traceFilterReader = eitherReader $ \s -> case s of
  "user-defined" -> Right UserDefined
  "compiler-generated" -> Right CompilerGenerated
  "all" -> Right All
  _ -> Left "Invalid trace filter; must be 'user-defined', 'compiler-generated', or 'all'"

formatReader :: ReadM Format
formatReader = eitherReader $ \s -> case s of
  "debruijn" -> Right DeBruijn
  _ -> Left "Invalid format; only 'debruijn' supported for now"
