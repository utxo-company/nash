{-# LANGUAGE OverloadedStrings #-}

module Main where

import Nash qualified as Nash
import PlutusCore.Default (DefaultFun, DefaultUni)
import PlutusCore.Evaluation.Machine.ExBudgetingDefaults
  ( defaultBuiltinCostModelForTesting,
    defaultCekMachineCostsForTesting,
  )
import PlutusCore.Evaluation.Machine.MachineParameters (CostModel (..), MachineParameters (..), mkMachineParameters)
import PlutusPrelude (def, pretty)
import System.Environment (getArgs)
import UntypedPlutusCore qualified as UPLC
import UntypedPlutusCore.Evaluation.Machine.Cek (CekValue, evaluateCekNoEmit)
import UntypedPlutusCore.Evaluation.Machine.Cek.CekMachineCosts (CekMachineCosts)

main :: IO ()
main = do
  args <- getArgs

  case args of
    [name] -> putStrLn $ Nash.greet name
    _ ->
      case eval
        ( UPLC.LamAbs
            ()
            (UPLC.Name "x" (UPLC.Unique 0))
            (UPLC.Var () (UPLC.Name "x" (UPLC.Unique 0)))
        ) of
        Left err -> putStrLn $ "Error: " ++ show err
        Right term -> putStrLn $ show (pretty term)
  where
    costModel =
      CostModel defaultCekMachineCostsForTesting defaultBuiltinCostModelForTesting
    machineParameters ::
      MachineParameters CekMachineCosts DefaultFun (CekValue DefaultUni DefaultFun ()) =
        mkMachineParameters def costModel
    eval =
      evaluateCekNoEmit machineParameters
