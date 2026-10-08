// Probe lexer for the Unicode version of an ANTLR tool jar (own code, MIT, lpg project).
lexer grammar U;
S: [\p{ID_Start}];
C: [\p{ID_Continue}];
