module Nash.Parse.Module where

import Data.ByteString qualified as BS
import Data.Name qualified as Name
import Data.Utf8 qualified
import Nash.Ast.Source qualified as Src
import Nash.Package qualified as Pkg
import Nash.Parse.Declaration qualified as Decl
import Nash.Parse.Primitives hiding (State, fromByteString)
import Nash.Parse.Primitives qualified as P
import Nash.Parse.Space qualified as Space
import Nash.Reporting.Annotation qualified as A
import Nash.Reporting.Error.Syntax qualified as E

fromByteString :: ProjectType -> BS.ByteString -> Either E.Error Src.Module
fromByteString projectType source =
  case P.fromByteString (chompModule projectType) E.ModuleBadEnd source of
    Right modul -> checkModule projectType modul
    Left err -> Left (E.ParseError err)

-- PROJECT TYPE

data ProjectType
  = Package Pkg.Name
  | Application

-- MODULE

data Module
  = Module
  { _header :: Maybe Header,
    _imports :: [Src.Import],
    _infixes :: [A.Located Src.Infix],
    _decls :: [Decl.Decl]
  }

chompModule :: ProjectType -> Parser E.Module Module
chompModule projectType =
  do
    header <- chompHeader
    imports <- chompImports (if isCore projectType then [] else Imports.defaults)
    infixes <- chompInfixes []
    decls <- specialize E.Declarations $ chompDecls []
    return (Module header imports infixes decls)

-- HEADER

data Header
  = Header
      (A.Located Name.Name)
      Effects
      (A.Located Src.Exposing)
      (Either A.Region Src.Comment)

data Effects
  = NoEffects A.Region
  | Ports A.Region
  | Manager A.Region Src.Manager

chompHeader :: Parser E.Module (Maybe Header)
chompHeader =
  do
    freshLine E.FreshLine
    start <- getPosition
    oneOfWithFallback
      [ -- module MyThing exposing (..)
        do
          Keyword.module_ E.ModuleProblem
          effectEnd <- getPosition
          Space.chompAndCheckIndent E.ModuleSpace E.ModuleProblem
          name <- addLocation (Var.moduleName E.ModuleName)
          Space.chompAndCheckIndent E.ModuleSpace E.ModuleProblem
          Keyword.exposing_ E.ModuleProblem
          Space.chompAndCheckIndent E.ModuleSpace E.ModuleProblem
          exports <- addLocation (specialize E.ModuleExposing exposing)
          comment <- chompModuleDocCommentSpace
          return $
            Just $
              Header name (NoEffects (A.Region start effectEnd)) exports comment,
        -- port module MyThing exposing (..)
        do
          Keyword.port_ E.PortModuleProblem
          Space.chompAndCheckIndent E.ModuleSpace E.PortModuleProblem
          Keyword.module_ E.PortModuleProblem
          effectEnd <- getPosition
          Space.chompAndCheckIndent E.ModuleSpace E.PortModuleProblem
          name <- addLocation (Var.moduleName E.PortModuleName)
          Space.chompAndCheckIndent E.ModuleSpace E.PortModuleProblem
          Keyword.exposing_ E.PortModuleProblem
          Space.chompAndCheckIndent E.ModuleSpace E.PortModuleProblem
          exports <- addLocation (specialize E.PortModuleExposing exposing)
          comment <- chompModuleDocCommentSpace
          return $
            Just $
              Header name (Ports (A.Region start effectEnd)) exports comment,
        -- effect module MyThing where { command = MyCmd } exposing (..)
        do
          Keyword.effect_ E.Effect
          Space.chompAndCheckIndent E.ModuleSpace E.Effect
          Keyword.module_ E.Effect
          effectEnd <- getPosition
          Space.chompAndCheckIndent E.ModuleSpace E.Effect
          name <- addLocation (Var.moduleName E.ModuleName)
          Space.chompAndCheckIndent E.ModuleSpace E.Effect
          Keyword.where_ E.Effect
          Space.chompAndCheckIndent E.ModuleSpace E.Effect
          manager <- chompManager
          Space.chompAndCheckIndent E.ModuleSpace E.Effect
          Keyword.exposing_ E.Effect
          Space.chompAndCheckIndent E.ModuleSpace E.Effect
          exports <- addLocation (specialize (const E.Effect) exposing)
          comment <- chompModuleDocCommentSpace
          return $
            Just $
              Header name (Manager (A.Region start effectEnd) manager) exports comment
      ]
      -- default header
      Nothing

freshLine :: (Row -> Col -> E.Module) -> Parser E.Module ()
freshLine toFreshLineError = do
  Space.chomp E.ModuleSpace
  Space.checkFreshLine toFreshLineError
