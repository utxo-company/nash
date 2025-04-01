{-# LANGUAGE OverloadedStrings #-}

module Main where

import Nash
import Options.Applicative
import PlutusCore.Default (DefaultFun (AddInteger), DefaultUni)
import PlutusCore.Evaluation.Machine.ExBudgetingDefaults (
    defaultBuiltinCostModelForTesting,
    defaultCekMachineCostsForTesting,
 )
import PlutusCore.Evaluation.Machine.MachineParameters (CostModel (..), MachineParameters (..), mkMachineParameters)
import PlutusCore.MkPlc (mkConstant)
import PlutusPrelude (def, pretty)
import UntypedPlutusCore qualified as UPLC
import UntypedPlutusCore.Evaluation.Machine.Cek (CekValue, evaluateCekNoEmit)
import UntypedPlutusCore.Evaluation.Machine.Cek.CekMachineCosts (CekMachineCosts)

main :: IO ()
main = execParser opts >>= handleCmd
    where
        opts =
            info
                (cmdParser <**> helper)
                ( fullDesc
                    <> header "Nash: a smart-contract language and toolchain for Cardano"
                )

randomUplc :: UPLC.Term UPLC.Name DefaultUni DefaultFun ()
randomUplc =
    UPLC.Apply
        ()
        ( UPLC.LamAbs
            ()
            (UPLC.Name "x" (UPLC.Unique 0))
            ( UPLC.Apply
                ()
                ( UPLC.Apply
                    ()
                    (UPLC.Builtin () AddInteger)
                    (UPLC.Var () (UPLC.Name "x" (UPLC.Unique 0)))
                )
                (mkConstant @Integer () 1)
            )
        )
        (mkConstant @Integer () 1)

runUplc :: UPLC.Term UPLC.Name DefaultUni DefaultFun () -> IO ()
runUplc term = do
    case eval term of
        Left err -> putStrLn $ "Error: " ++ show err
        Right term' -> putStrLn $ show (pretty term')
    where
        costModel =
            CostModel defaultCekMachineCostsForTesting defaultBuiltinCostModelForTesting
        machineParameters ::
            MachineParameters CekMachineCosts DefaultFun (CekValue DefaultUni DefaultFun ()) =
                mkMachineParameters def costModel
        eval =
            evaluateCekNoEmit machineParameters
