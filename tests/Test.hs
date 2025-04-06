module Main where

import Data.ByteString qualified as BS
import Nash.Parse.Module qualified as M
import System.Directory (createDirectoryIfMissing)
import Test.Tasty
import Test.Tasty.Golden
import Text.Show.Pretty (ppShow)

main :: IO ()
main = defaultMain tests

tests :: TestTree
tests =
    testGroup
        "Parser Tests"
        [ goldenParse "Parse module" "module"
        ]

goldenParse :: String -> String -> TestTree
goldenParse name base =
    goldenVsFile
        name
        ("tests/golden/parser/" ++ base ++ ".txt") -- Golden file
        ("tests/output/parser/" ++ base ++ ".out") -- Output file
        $ do
            createDirectoryIfMissing True ("tests/output/parser")

            input <- BS.readFile ("tests/input/parser/" ++ base ++ ".ns")

            let result = M.fromByteString M.Application input

            let output = case result of
                    Right m -> ppShow m
                    Left e -> ppShow e

            writeFile ("tests/output/parser/" ++ base ++ ".out") output
