module Main where

import Data.ByteString.Lazy.Char8 qualified as BS
import Test.Tasty
import Test.Tasty.Golden

main :: IO ()
main = defaultMain tests

tests :: TestTree
tests =
    testGroup
        "Parser Tests"
        [ goldenVsString "Dummy Parse" "tests/golden/dummy.txt" $ pure $ BS.pack "Hello, World!"
        ]
