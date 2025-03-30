module Nash.Terminal.Test where

import Options.Applicative

data TestArgs = TestArgs
  { testDirectory :: Maybe FilePath,
    testWatch :: Bool
  }
  deriving (Show)

testArgsParser :: Parser TestArgs
testArgsParser =
  TestArgs
    <$> optional
      ( argument
          str
          ( metavar "DIRECTORY"
              <> help "Path to project"
          )
      )
    <*> switch
      ( long "watch"
          <> short 'w'
          <> help "Watch mode"
      )
