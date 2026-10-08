| Needs | Status | Rules |
|---|---|---|
| mandatory | supported | `gqlProgram`, `programActivity`, `sessionActivity`, `transactionActivity`, `sessionSetCommand`, `sessionSetSchemaClause`, `sessionSetGraphClause`, `sessionCloseCommand`, `procedureSpecification`, `nestedDataModifyingProcedureSpecification`, `nestedQuerySpecification`, `procedureBody`, `statementBlock`, `statement`, `graphExpression`, `currentGraph`, `objectExpressionPrimary`, `linearDataModifyingStatement`, `ambientLinearDataModifyingStatement`, `ambientLinearDataModifyingStatementBody`, `simpleLinearDataAccessingStatement`, `simpleDataAccessingStatement`, `compositeQueryStatement`, `compositeQueryExpression`, `compositeQueryPrimary`, `linearQueryStatement`, `focusedLinearQueryStatement`, `ambientLinearQueryStatement`, `simpleLinearQueryStatement`, `simpleQueryStatement`, `primitiveQueryStatement`, `matchStatement`, `simpleMatchStatement`, `optionalMatchStatement`, `optionalOperand`, `matchStatementBlock`, `orderByAndPageStatement`, `primitiveResultStatement`, `returnStatement`, `returnStatementBody`, `returnItemList`, `returnItem`, `returnItemAlias`, `selectStatement`, `selectItemList`, `selectItem`, `selectItemAlias`, `havingClause`, `selectStatementBody`, `selectGraphMatchList`, `selectGraphMatch`, `selectQuerySpecification`, `graphPatternBindingTable`, `graphPattern`, `pathPatternList`, `pathPattern`, `graphPatternWhereClause`, `pathPatternExpression`, `pathTerm`, `pathFactor`, `pathPrimary`, `elementPattern`, `nodePattern`, `elementPatternFiller`, `elementVariableDeclaration`, `isLabelExpression`, `isOrColon`, `elementPatternPredicate`, `elementPatternWhereClause`, `elementPropertySpecification`, `propertyKeyValuePairList`, `propertyKeyValuePair`, `edgePattern`, `fullEdgePattern`, `fullEdgePointingRight`, `labelExpression`, `elementVariableReference`, `whereClause`, `orderByClause`, `sortSpecificationList`, `sortSpecification`, `sortKey`, `orderingSpecification`, `schemaReference`, `absoluteCatalogSchemaReference`, `relativeCatalogSchemaReference`, `predefinedSchemaReference`, `absoluteDirectoryPath`, `relativeDirectoryPath`, `simpleDirectoryPath`, `graphReference`, `homeGraph`, `catalogObjectParentReference`, `searchCondition`, `predicate`, `compOp`, `existsPredicate`, `nullPredicate`, `nullPredicatePart2`, `normalizedPredicatePart2`, `valueExpression`, `valueFunction`, `booleanValueExpression`, `characterOrByteStringFunction`, `subCharacterOrByteString`, `trimSingleCharacterOrByteString`, `foldCharacterString`, `normalizeCharacterString`, `aggregatingValueExpression`, `valueExpressionPrimary`, `parenthesizedValueExpression`, `nonParenthesizedValueExpressionPrimary`, `nonParenthesizedValueExpressionPrimarySpecialCase`, `unsignedValueSpecification`, `generalValueSpecification`, `dynamicParameterSpecification`, `caseExpression`, `caseAbbreviation`, `caseSpecification`, `simpleCase`, `searchedCase`, `simpleWhenClause`, `searchedWhenClause`, `elseClause`, `caseOperand`, `whenOperandList`, `whenOperand`, `result`, `resultExpression`, `aggregateFunction`, `generalSetFunction`, `generalSetFunctionType`, `setQuantifier`, `bindingVariableReference`, `truthValue`, `numericValueExpression`, `numericValueFunction`, `lengthExpression`, `charLengthExpression`, `characterStringValueExpression`, `trimOperands`, `trimCharacterOrByteStringSource`, `normalForm`, `stringLength`, `objectName`, `objectNameOrBindingVariable`, `directoryName`, `schemaName`, `graphName`, `delimitedGraphName`, `labelName`, `propertyName`, `elementVariable`, `bindingVariable`, `unsignedLiteral`, `generalLiteral`, `identifier`, `regularIdentifier`, `characterStringLiteral`, `unsignedNumericLiteral`, `exactNumericLiteral`, `approximateNumericLiteral`, `unsignedInteger`, `nullLiteral`, `nonReservedWords` (165) |
| G002 | not supported | `differentEdgesMatchMode` (1) |
| G002 or G003 | not supported | `matchMode` (1) |
| G003 | not supported | `repeatableElementsMatchMode` (1) |
| G004 | not supported | `pathVariableDeclaration` (1) |
| G005+G006+G015 or G005+G006+G016 or G005+G006+G017 or G005+G006+G018 or G005+G006+G019 or G005+G006+G020 or G005+G007+G015 or G005+G007+G016 or G005+G007+G017 or G005+G007+G018 or G005+G007+G019 or G005+G007+G020 or G006+G010 or G006+G011 or G006+G012 or G006+G013 or G007+G010 or G007+G011 or G007+G012 or G007+G013 | not supported | `keepClause` (1) |
| G005+G015 or G005+G016 or G005+G017 or G005+G018 or G005+G019 or G005+G020 | not supported | `pathSearchPrefix` (1) |
| G005+G015 or G005+G016 or G005+G017 or G005+G018 or G005+G019 or G005+G020 or G010 or G011 or G012 or G013 | not supported | `pathPatternPrefix` (1) |
| G010 or G011 or G012 or G013 | not supported | `pathModePrefix`, `pathMode` (2) |
| G014 | not supported | `pathOrPaths` (1) |
| G015; inside G005 | not supported | `allPathSearch` (1) |
| G016; inside G005 | not supported | `anyPathSearch` (1) |
| G017 or G018 or G019 or G020; inside G005 | not supported | `shortestPathSearch` (1) |
| G017; inside G005 | not supported | `allShortestPathSearch` (1) |
| G018; inside G005 | not supported | `anyShortestPathSearch` (1) |
| G019; inside G005 | not supported | `countedShortestPathSearch` (1) |
| G020; inside G005 | not supported | `countedShortestGroupSearch` (1) |
| G030; inside G039, G080 | not supported | `simplifiedMultisetAlternation` (1) |
| G032; inside G039, G080 | not supported | `simplifiedPathUnion` (1) |
| G035+G060 or G035+G061 or G036+G060 or G036+G061; inside G039, G080 | not supported | `simplifiedQuantified` (1) |
| G037; inside G039, G080 | not supported | `simplifiedQuestioned` (1) |
| G038 | not supported | `parenthesizedPathPatternExpression` (1) |
| G039 or G080 | not supported | `simplifiedPathPatternExpression` (1) |
| G043 | not supported | `fullEdgePointingLeft`, `fullEdgeLeftOrRight`, `fullEdgeAnyDirection` (3) |
| G043+GH02 | not supported | `fullEdgeLeftOrUndirected`, `fullEdgeUndirectedOrRight` (2) |
| G044 or G045 | not supported | `abbreviatedEdgePattern` (1) |
| G048; inside G038 | not supported | `subpathVariableDeclaration` (1) |
| G050; inside G038 | not supported | `parenthesizedPathPatternWhereClause` (1) |
| G060 or G061; inside G035, G036 | not supported | `graphPatternQuantifier`, `generalQuantifier` (2) |
| G081 or G082; inside G039, G080 | not supported | `simplifiedDirectionOverride` (1) |
| G100 | not supported | `element_idFunction` (1) |
| G110 | not supported | `directedPredicate`, `directedPredicatePart2` (2) |
| G111 | not supported | `labeledPredicate`, `labeledPredicatePart2` (2) |
| G112 | not supported | `sourceDestinationPredicate`, `sourcePredicatePart2`, `destinationPredicatePart2` (3) |
| G113 | not supported | `all_differentPredicate` (1) |
| G114 | not supported | `samePredicate` (1) |
| G115 | not supported | `property_existsPredicate` (1) |
| GA03 | not supported | `nullOrdering` (1) |
| GA05 | not supported | `castSpecification` (1) |
| GA06 | not supported | `valueTypePredicate`, `valueTypePredicatePart2` (2) |
| GC01 | supported | `createSchemaStatement`, `dropSchemaStatement`, `createGraphTypeStatement`, `dropGraphTypeStatement` (4) |
| GC01 or GC04 | supported | `primitiveCatalogModifyingStatement` (1) |
| GC01 or GC04 or GP01 or GP04 | supported | `linearCatalogModifyingStatement`, `simpleCatalogModifyingStatement` (2) |
| GC04 | supported | `dropGraphStatement` (1) |
| GC04+GG01 or GC04+GG02 or GC04+GG04 | supported | `createGraphStatement` (1) |
| GD01 | supported | `primitiveDataModifyingStatement`, `insertStatement`, `setStatement`, `removeStatement` (4) |
| GD01 or GP01 or GP04 | supported | `simpleDataModifyingStatement` (1) |
| GD01+GD03 or GD01+GD04 | not supported | `deleteStatement` (1) |
| GD02; inside GD01 | not supported | `setLabelItem`, `removeLabelItem` (2) |
| GD03 or GD04; inside GD01 | not supported | `deleteItemList`, `deleteItem` (2) |
| GE03 | not supported | `letValueExpression` (1) |
| GE06 | not supported | `pathValueConstructor` (1) |
| GE08 | not supported | `referenceParameterSpecification` (1) |
| GF04 | not supported | `elementsFunction` (1) |
| GF04 or GV50 | not supported | `listValueFunction` (1) |
| GF05 | not supported | `trimMultiCharacterCharacterString` (1) |
| GF06 | not supported | `trimSpecification`, `trimCharacterOrByteString` (2) |
| GF11 | not supported | `binarySetFunction` (1) |
| GF12 or GF13 | not supported | `cardinalityExpression` (1) |
| GG01; inside GC04 | supported | `openGraphType` (1) |
| GG02 or GG04; inside GC04 | supported | `ofGraphType` (1) |
| GG03; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `closedGraphReferenceValueType` (1) |
| GG03; inside GA05, GA06, GC01, GC04, GE03, GG02, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `nestedGraphTypeSpecification` (1) |
| GG04; inside GC01, GC04 | not supported | `graphTypeLikeGraph` (1) |
| GG05; inside GC04 | not supported | `graphSource` (1) |
| GG20; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `nodeTypeName`, `edgeTypeName` (2) |
| GG21; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `nodeTypeKeyLabelSet`, `edgeTypeKeyLabelSet` (2) |
| GH02 | not supported | `fullEdgeUndirected` (1) |
| GH02; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `edgeTypePatternUndirected`, `endpointPairUndirected` (2) |
| GH02; inside GD01 | not supported | `insertEdgeUndirected` (1) |
| GP01 | not supported | `inlineProcedureCall` (1) |
| GP01 or GP04 | supported | `callCatalogModifyingProcedureStatement`, `callDataModifyingProcedureStatement`, `callQueryStatement`, `callProcedureStatement`, `procedureCall` (5) |
| GP03; inside GP01 | not supported | `variableScopeClause` (1) |
| GP04 | supported | `namedProcedureCall` (1) |
| GP05 or GP08 or GP11; inside GP17 | not supported | `bindingVariableDefinition` (1) |
| GP05+GP17 or GP08+GP17 or GP11+GP17 | not supported | `bindingVariableDefinitionBlock` (1) |
| GP16 | not supported | `atSchemaClause` (1) |
| GQ01 | supported | `focusedLinearDataModifyingStatement`, `focusedLinearDataModifyingStatementBody`, `focusedNestedDataModifyingProcedureSpecification`, `focusedLinearQueryStatementPart`, `focusedLinearQueryAndPrimitiveResultStatementPart`, `focusedPrimitiveResultStatement`, `focusedNestedQuerySpecification`, `useGraphClause` (8) |
| GQ02 or GQ03 or GQ04 or GQ05 or GQ06 or GQ07 | not supported | `queryConjunction` (1) |
| GQ03 or GQ04 or GQ05 or GQ06 or GQ07 | not supported | `setOperator` (1) |
| GQ08 | not supported | `filterStatement` (1) |
| GQ09 | not supported | `letStatement` (1) |
| GQ10 or GQ23 | not supported | `forStatement` (1) |
| GQ11 or GQ24; inside GQ10, GQ23 | not supported | `forOrdinalityOrOffset` (1) |
| GQ12 | not supported | `offsetClause` (1) |
| GQ13 | not supported | `limitClause` (1) |
| GQ15 | not supported | `groupByClause` (1) |
| GQ18 | not supported | `valueQueryExpression` (1) |
| GQ19 | not supported | `graphPatternYieldClause` (1) |
| GQ20 | not supported | `nextStatement` (1) |
| GS01 | not supported | `sessionSetGraphParameterClause` (1) |
| GS01 or GS02 or GS03 | not supported | `sessionSetParameterClause` (1) |
| GS02 | not supported | `sessionSetBindingTableParameterClause` (1) |
| GS03 | not supported | `sessionSetValueParameterClause` (1) |
| GS04 or GS05 or GS06 or GS07 or GS08 or GS16 | not supported | `sessionResetCommand` (1) |
| GS15 | not supported | `sessionSetTimeZoneClause` (1) |
| GT01 | supported | `endTransactionCommand`, `startTransactionCommand`, `rollbackCommand`, `commitCommand` (4) |
| GT02; inside GT01 | not supported | `transactionCharacteristics` (1) |
| GV17; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `decimalExactNumericType` (1) |
| GV35; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `byteStringType` (1) |
| GV39 | supported | `dateFunction`, `localtimeFunction`, `localdatetimeFunction`, `dateLiteral` (4) |
| GV39 or GV40 | supported | `datetimeValueFunction`, `temporalLiteral` (2) |
| GV39 or GV40 or GV41; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `temporalType` (1) |
| GV39 or GV40; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `temporalInstantType` (1) |
| GV39; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `localdatetimeType`, `dateType`, `localtimeType` (3) |
| GV40 | supported | `timeFunction`, `datetimeFunction`, `timeLiteral`, `datetimeLiteral` (4) |
| GV40; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `datetimeType`, `timeType` (2) |
| GV41 | supported | `datetimeSubtraction`, `durationValueFunction`, `durationFunction`, `durationLiteral` (4) |
| GV41; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `temporalDurationType` (1) |
| GV45 | not supported | `recordLiteral` (1) |
| GV46 or GV47; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `recordType` (1) |
| GV50 | not supported | `trimListFunction`, `listValueConstructorByEnumeration`, `listLiteral` (3) |
| GV55; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `pathValueType` (1) |
| GV60; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV61 | not supported | `graphReferenceValueType` (1) |
| GV61; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60 | not supported | `bindingTableReferenceValueType` (1) |
| GV71 or GV72; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `immaterialValueType` (1) |
| GV71; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `nullType` (1) |
| GV72; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `emptyType` (1) |
| GV90; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `notNull` (1) |
| inside G002 | not supported | `edgeBindingsOrEdges`, `edgesSynonym` (2) |
| inside G002, GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `edgeSynonym` (1) |
| inside G003 | not supported | `elementBindingsOrElements` (1) |
| inside G004 | not supported | `pathVariable` (1) |
| inside G005, G016, G019 | not supported | `numberOfPaths` (1) |
| inside G005, G016, G019, G020, GQ12, GQ13 | not supported | `nonNegativeIntegerSpecification` (1) |
| inside G005, G020 | not supported | `numberOfGroups` (1) |
| inside G035, G036, G060 | not supported | `fixedQuantifier` (1) |
| inside G035, G036, G060, G061 | not supported | `lowerBound`, `upperBound` (2) |
| inside G038, G048 | not supported | `subpathVariable` (1) |
| inside G039, G080 | not supported | `simplifiedDefaultingLeft`, `simplifiedDefaultingUndirected`, `simplifiedDefaultingRight`, `simplifiedDefaultingLeftOrUndirected`, `simplifiedDefaultingUndirectedOrRight`, `simplifiedDefaultingLeftOrRight`, `simplifiedDefaultingAnyDirection`, `simplifiedContents`, `simplifiedTerm`, `simplifiedFactorLow`, `simplifiedFactorHigh`, `simplifiedTertiary`, `simplifiedSecondary`, `simplifiedNegation`, `simplifiedPrimary` (15) |
| inside G039, G080, G081, G082 | not supported | `simplifiedOverrideLeft`, `simplifiedOverrideUndirected`, `simplifiedOverrideRight`, `simplifiedOverrideLeftOrUndirected`, `simplifiedOverrideUndirectedOrRight`, `simplifiedOverrideLeftOrRight`, `simplifiedOverrideAnyDirection` (7) |
| inside G111 | not supported | `isLabeledOrColon` (1) |
| inside G112 | not supported | `nodeReference`, `edgeReference` (2) |
| inside GA05 | not supported | `castOperand`, `castTarget` (2) |
| inside GA05, GA06, GC01, GC04, GD01, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `labelSetSpecification` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG01, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03 | supported | `typed` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GH02, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `arcTypeUndirected`, `connectorUndirected` (2) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP04, GP05, GP08, GP17, GQ09, GQ20, GS01, GS02, GS03, GV39, GV40, GV41, GV45, GV46, GV60, GV61 | supported | `fieldName` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `openGraphReferenceValueType` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `nodeTypeSpecification`, `nodeTypePattern`, `nodeTypePhrase`, `nodeTypePhraseFiller`, `nodeTypeFiller`, `localNodeTypeAlias`, `nodeTypeImpliedContent`, `nodeTypeLabelSet`, `nodeTypePropertyTypes`, `edgeTypeSpecification`, `edgeTypePattern`, `edgeTypePhrase`, `edgeTypePhraseFiller`, `edgeTypeFiller`, `edgeTypeImpliedContent`, `edgeTypeLabelSet`, `edgeTypePropertyTypes`, `edgeTypePatternDirected`, `edgeTypePatternPointingRight`, `edgeTypePatternPointingLeft`, `arcTypePointingRight`, `arcTypePointingLeft`, `sourceNodeTypeReference`, `destinationNodeTypeReference`, `edgeKind`, `endpointPairPhrase`, `endpointPair`, `endpointPairDirected`, `endpointPairPointingRight`, `endpointPairPointingLeft`, `connectorPointingRight`, `sourceNodeTypeAlias`, `destinationNodeTypeAlias`, `labelSetPhrase`, `propertyTypesSpecification`, `propertyTypeList`, `propertyType`, `propertyValueType`, `valueType`, `predefinedType`, `booleanType`, `characterStringType`, `minLength`, `maxLength`, `numericType`, `exactNumericType`, `binaryExactNumericType`, `signedBinaryExactNumericType`, `unsignedBinaryExactNumericType`, `verboseBinaryExactNumericType`, `precision`, `scale`, `approximateNumericType`, `referenceValueType`, `nodeReferenceValueType`, `closedNodeReferenceValueType`, `openNodeReferenceValueType`, `edgeReferenceValueType`, `closedEdgeReferenceValueType`, `openEdgeReferenceValueType`, `unsignedDecimalInteger`, `nodeSynonym` (62) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV32, GV35, GV60, GV61 | not supported | `fixedLength` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV46, GV60, GV61 | not supported | `fieldTypesSpecification`, `fieldTypeList`, `fieldType` (3) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `bindingTableType` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `graphTypeSpecificationBody`, `elementTypeList`, `elementTypeSpecification` (3) |
| inside GC01 | supported | `graphTypeSource`, `copyOfGraphType`, `catalogSchemaParentAndName` (3) |
| inside GC01, GC04, GG02 | supported | `graphTypeReference`, `catalogGraphTypeParentAndName`, `graphTypeName` (3) |
| inside GC04 | supported | `catalogGraphParentAndName` (1) |
| inside GD01 | supported | `setItemList`, `setItem`, `setPropertyItem`, `setAllPropertiesItem`, `removeItemList`, `removeItem`, `removePropertyItem`, `insertGraphPattern`, `insertPathPatternList`, `insertPathPattern`, `insertNodePattern`, `insertEdgePattern`, `insertEdgePointingLeft`, `insertEdgePointingRight`, `insertElementPatternFiller`, `labelAndPropertySetSpecification` (16) |
| inside GE02, GP08, GP17, GS02 | not supported | `bindingTableExpression`, `nestedBindingTableQuerySpecification`, `bindingTableReference`, `bindingTableName`, `delimitedBindingTableName` (5) |
| inside GE03, GP05, GP17, GQ09 | not supported | `valueVariableDefinition` (1) |
| inside GE03, GP05, GP17, GQ09, GS03 | not supported | `optTypedValueInitializer`, `valueInitializer` (2) |
| inside GE03, GQ09 | not supported | `letVariableDefinitionList`, `letVariableDefinition` (2) |
| inside GE06 | not supported | `nodeReferenceValueExpression`, `edgeReferenceValueExpression`, `pathValueConstructorByEnumeration`, `pathElementList`, `pathElementListStart`, `pathElementListStep` (6) |
| inside GF01 | not supported | `modulusExpression`, `numericValueExpressionDividend`, `numericValueExpressionDivisor`, `squareRoot`, `floorFunction`, `ceilingFunction` (6) |
| inside GF01, GF03 | not supported | `exponentialFunction`, `powerFunction`, `numericValueExpressionBase`, `numericValueExpressionExponent` (4) |
| inside GF01, GV41 | supported | `absoluteValueExpression` (1) |
| inside GF02 | not supported | `trigonometricFunction`, `trigonometricFunctionName` (2) |
| inside GF03 | not supported | `generalLogarithmFunction`, `generalLogarithmBase`, `generalLogarithmArgument`, `commonLogarithm`, `naturalLogarithm` (5) |
| inside GF04 | not supported | `pathValueExpression`, `pathLengthExpression` (2) |
| inside GF11 | not supported | `binarySetFunctionType`, `dependentValueExpression`, `independentValueExpression` (3) |
| inside GF12 | not supported | `cardinalityExpressionArgument` (1) |
| inside GF13, GV50 | not supported | `listValueExpression` (1) |
| inside GP01 | not supported | `nestedProcedureSpecification` (1) |
| inside GP01, GP03 | not supported | `bindingVariableReferenceList` (1) |
| inside GP04 | supported | `procedureArgumentList`, `procedureArgument`, `procedureReference`, `catalogProcedureParentAndName`, `procedureName` (5) |
| inside GP04, GQ20 | supported | `yieldClause`, `yieldItemList`, `yieldItem`, `yieldItemName`, `yieldItemAlias` (5) |
| inside GP08, GP17 | not supported | `bindingTableVariableDefinition` (1) |
| inside GP08, GP17, GS02 | not supported | `optTypedBindingTableInitializer`, `bindingTableInitializer` (2) |
| inside GP11, GP17 | not supported | `graphVariableDefinition` (1) |
| inside GP11, GP17, GS01 | not supported | `optTypedGraphInitializer`, `graphInitializer` (2) |
| inside GQ10, GQ23 | not supported | `forItem`, `forItemAlias`, `forItemSource` (3) |
| inside GQ12 | not supported | `offsetSynonym` (1) |
| inside GQ15 | not supported | `groupingElementList`, `groupingElement`, `emptyGroupingSet` (3) |
| inside GQ19 | not supported | `graphPatternYieldItemList`, `graphPatternYieldItem` (2) |
| inside GS01, GS02, GS03 | not supported | `sessionSetParameterName` (1) |
| inside GS01, GS02, GS03, GS04, GS05, GS06, GS07, GS08, GS16 | not supported | `sessionParameterSpecification` (1) |
| inside GS04, GS05, GS06, GS07, GS08, GS16 | not supported | `sessionResetArguments` (1) |
| inside GS15 | not supported | `setTimeZoneValue`, `timeZoneString` (2) |
| inside GT01, GT02 | not supported | `transactionMode`, `transactionAccessMode` (2) |
| inside GV35 | not supported | `byteLengthExpression`, `byteStringValueExpression` (2) |
| inside GV39 | supported | `dateFunctionParameters`, `dateString` (2) |
| inside GV39, GV40 | supported | `timeFunctionParameters`, `datetimeFunctionParameters`, `timeString`, `datetimeString` (4) |
| inside GV39, GV40, GV41, GV45 | supported | `recordConstructor`, `fieldsSpecification`, `fieldList`, `field` (4) |
| inside GV41 | supported | `temporalDurationQualifier`, `datetimeValueExpression`, `datetimeSubtractionParameters`, `datetimeValueExpression1`, `datetimeValueExpression2`, `durationFunctionParameters`, `durationString` (7) |
| inside GV50 | not supported | `listValueTypeName`, `listValueTypeNameSynonym`, `listElementList`, `listElement` (4) |
| unreachable from gqlProgram | never parsed | `pathVariableReference`, `listValueConstructor`, `durationValueExpression` (3) |

| Partial anchors inside supported rules | Rule |
|---|---|
| GC02 (IF) | `createSchemaStatement` |
| GC02 (IF) | `dropSchemaStatement` |
| GC05 (IF) | `createGraphStatement` |
| GG02 (alternative 2); GG02 (alternative 3) | `ofGraphType` |
| GC05 (IF) | `dropGraphStatement` |
| GC03 (IF) | `createGraphTypeStatement` |
| GC03 (IF) | `dropGraphTypeStatement` |
| G030 (alternative 2); G032 (alternative 3) | `pathPatternExpression` |
| G035 or G036 (alternative 2); G037 (alternative 3) | `pathFactor` |
| G074 (alternative 5) | `labelExpression` |
| GV50 (alternative 3); GV50 (alternative 4); GV50 (alternative 5); GV66 (alternative 7); GV67 (alternative 10); GV67 (alternative 9); GV68 (alternative 8) | `valueType` |
| GV30 (minLength); GV31 (alternative 3); GV31 (maxLength); GV32 (alternative 2) | `characterStringType` |
| GV02 (alternative 1); GV04 (alternative 2); GV07 (alternative 3); GV09 (precision); GV12 (alternative 4); GV14 (alternative 5); GV16 (alternative 6); GV18 (alternative 7); GV19 (alternative 9) | `signedBinaryExactNumericType` |
| GV01 (alternative 1); GV03 (alternative 2); GV05 (alternative 7); GV06 (alternative 3); GV08 (alternative 8); GV09 (precision); GV10 (alternative 9); GV11 (alternative 4); GV13 (alternative 5); GV15 (alternative 6) | `unsignedBinaryExactNumericType` |
| GV02 (alternative 1); GV04 (alternative 2); GV07 (alternative 3); GV09 (precision); GV12 (alternative 4); GV14 (alternative 5); GV16 (alternative 6); GV18 (alternative 7); GV19 (alternative 9) | `verboseBinaryExactNumericType` |
| GV20 (alternative 1); GV21 (alternative 2); GV22 (precision); GV23 (alternative 7); GV23 (alternative 8); GV24 (alternative 3); GV25 (alternative 4); GV26 (alternative 5) | `approximateNumericType` |
| GE01 (alternative 12); GE02 (alternative 13); GE07 (XOR) | `valueExpression` |
| GF10 (alternative 6) | `generalSetFunctionType` |
| GF01 (alternative 11); GF01 (alternative 12); GF01 (alternative 13); GF01 (alternative 3); GF01 (alternative 4); GF01 or GF03 (alternative 10); GF01 or GF03 (alternative 9); GF02 (alternative 5); GF03 (alternative 6); GF03 (alternative 7); GF03 (alternative 8) | `numericValueFunction` |
| GF04 (alternative 3); GV35 (alternative 2) | `lengthExpression` |
| GV41 (alternative 2) | `durationValueFunction` |
| GV35 (alternative 3) | `generalLiteral` |
| GL04 (alternative 3); GL05 (alternative 2); GL05 (alternative 4); GL06 (alternative 1) | `exactNumericLiteral` |
| GL07 (alternative 3); GL07 (alternative 4); GL08 (alternative 1) | `approximateNumericLiteral` |
| GL01 (alternative 2); GL02 (alternative 3); GL03 (alternative 4) | `unsignedInteger` |
