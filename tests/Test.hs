module Main where

import Data.ByteString qualified as BS
import Data.ByteString.Lazy.Char8 qualified as BSL
import Nash.Parse.Module qualified as M
import Test.Tasty
import Test.Tasty.Golden

main :: IO ()
main = defaultMain tests

tests :: TestTree
tests =
    testGroup
        "Parser Tests"
        [ goldenVsString "Dummy Parse" "tests/golden/dummy.txt" $ pure $ BSL.pack "Hello, World!"
        , goldenParse "Parse module" "module"
        ]
    where
        goldenParse :: String -> FilePath -> TestTree
        goldenParse name base =
            goldenVsFile
                name
                ("tests/golden/parser/" ++ base ++ ".txt") -- Golden file
                ("tests/output/parser/" ++ base ++ ".out") -- Output file
                $ do
                    input <- BS.readFile ("tests/input/parser/" ++ base ++ ".nash")

                    let result = M.fromByteString M.Application input

                    let output = show result

                    writeFile ("tests/output/parser/" ++ base ++ ".out") output
