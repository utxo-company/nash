module Nash.Cmd where

import Nash.Terminal.Build (BuildArgs)
import Nash.Terminal.Check (CheckArgs)
import Nash.Terminal.Fmt (FmtArgs)
import Nash.Terminal.Parsers (Cmd (..))
import Nash.Terminal.Test (TestArgs)
import Nash.Terminal.Uplc (UplcCmd (..))

handleCmd :: Cmd -> IO ()
handleCmd cmd = case cmd of
    Fmt args -> handleFmt args
    Build args -> handleBuild args
    Check args -> handleCheck args
    Uplc uplcCmd -> handleUplc uplcCmd
    Test testArgs -> handleTest testArgs

handleFmt :: FmtArgs -> IO ()
handleFmt _ = putStrLn "Formatting project (dummy implementation)"

handleBuild :: BuildArgs -> IO ()
handleBuild _ = putStrLn "Building project (dummy implementation)"

handleCheck :: CheckArgs -> IO ()
handleCheck _ = putStrLn "Checking project (dummy implementation)"

handleTest :: TestArgs -> IO ()
handleTest _ = putStrLn "Testing project (dummy implementation)"

handleUplc :: UplcCmd -> IO ()
handleUplc cmd = case cmd of
    UplcFmt _ -> putStrLn "Formatting UPLC (dummy implementation)"
    UplcEval _ -> putStrLn "Evaluating UPLC (dummy implementation)"
    UplcEncode _ -> putStrLn "Encoding UPLC (dummy implementation)"
    UplcDecode _ -> putStrLn "Decoding UPLC (dummy implementation)"
    UplcShrink _ -> putStrLn "Shrinking UPLC (dummy implementation)"
