{-# LANGUAGE ApplicativeDo #-}

module Nash.Terminal.Parsers where

import Nash.Terminal.Build (BuildArgs, buildArgsParser)
import Nash.Terminal.Check (CheckArgs, checkArgsParser)
import Nash.Terminal.Fmt (FmtArgs, fmtArgsParser)
import Nash.Terminal.Test (TestArgs, testArgsParser)
import Nash.Terminal.Uplc
  ( UplcCmd (..),
    uplcDecodeArgsParser,
    uplcEncodeArgsParser,
    uplcEvalArgsParser,
    uplcFmtArgsParser,
    uplcShrinkArgsParser,
  )
import Options.Applicative

data Cmd
  = Fmt FmtArgs
  | Build BuildArgs
  | Check CheckArgs
  | Uplc UplcCmd
  | Test TestArgs
  deriving (Show)

cmdParser :: Parser Cmd
cmdParser =
  subparser
    ( command
        "fmt"
        ( info
            (Fmt <$> fmtArgsParser <**> helper)
            (progDesc "Format a nash project")
        )
        <> command
          "build"
          ( info
              (Build <$> buildArgsParser <**> helper)
              (progDesc "Build a nash project")
          )
        <> command
          "check"
          ( info
              (Check <$> checkArgsParser <**> helper)
              (progDesc "Type-check a nash project and run tests")
          )
        <> command
          "uplc"
          ( info
              (Uplc <$> uplcCmdParser <**> helper)
              (progDesc "Commands for working with untyped Plutus-core")
          )
        <> command
          "test"
          ( info
              (Test <$> testArgsParser <**> helper)
              (progDesc "Run tests for a nash project")
          )
    )
    <**> infoOption "version placeholder" (long "version" <> help "Show version")

uplcCmdParser :: Parser UplcCmd
uplcCmdParser =
  subparser
    ( command
        "fmt"
        ( info
            (UplcFmt <$> uplcFmtArgsParser <**> helper)
            (progDesc "Format an Untyped Plutus Core program")
        )
        <> command
          "eval"
          ( info
              (UplcEval <$> uplcEvalArgsParser <**> helper)
              (progDesc "Evaluate an Untyped Plutus Core program")
          )
        <> command
          "encode"
          ( info
              (UplcEncode <$> uplcEncodeArgsParser <**> helper)
              (progDesc "Encode textual Untyped Plutus Core to flat bytes")
          )
        <> command
          "decode"
          ( info
              (UplcDecode <$> uplcDecodeArgsParser <**> helper)
              (progDesc "Decode flat bytes to textual Untyped Plutus Core")
          )
        <> command
          "optimize"
          ( info
              (UplcShrink <$> uplcShrinkArgsParser <**> helper)
              (progDesc "Shrink/Optimize UPLC code")
          )
    )
