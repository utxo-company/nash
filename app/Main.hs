module Main where

import Nash
import System.Environment (getArgs)

main :: IO ()
main = do
  args <- getArgs

  case args of
    [name] -> putStrLn $ greet name
    _ -> putStrLn "Usage: nash <name>"
