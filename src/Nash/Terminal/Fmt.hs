module Nash.Terminal.Fmt where

import Options.Applicative

data FmtArgs = FmtArgs
    { fmtFiles :: [String]
    , fmtStdin :: Bool
    , fmtCheck :: Bool
    }
    deriving (Show)

fmtArgsParser :: Parser FmtArgs
fmtArgsParser =
    FmtArgs
        <$> ( many
                ( argument
                    str
                    ( metavar "FILES"
                        <> help "Files to format"
                    )
                )
                <|> pure ["."]
            )
        <*> switch
            ( long "stdin"
                <> help "Read source from STDIN"
            )
        <*> switch
            ( long "check"
                <> help "Check if inputs are formatted without changing them"
            )
