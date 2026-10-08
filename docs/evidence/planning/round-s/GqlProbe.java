// Parse probe for TCK selection: the ANTLR 4 Java target generated from the unmodified GQL.g4.
// Input: one file of NUL-separated GQL texts. Output: one line per text:
//   OK <items>   or   ERR
// With a second argument "ambig", prediction runs in LL_EXACT_AMBIG_DETECTION mode and each
// OK line also lists "ambig:<rule>/<alternatives>" for every ambiguity ANTLR reports.
// Items: rule, rule/N (outer alternative), rule/#label (labelled alternative),
// rule:ELEMENT (each child token symbol or child rule). Own code (MIT, lpg project).
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.BitSet;
import java.util.TreeSet;
import org.antlr.v4.runtime.*;
import org.antlr.v4.runtime.atn.ATNConfigSet;
import org.antlr.v4.runtime.atn.PredictionMode;
import org.antlr.v4.runtime.dfa.DFA;
import org.antlr.v4.runtime.tree.ParseTree;
import org.antlr.v4.runtime.tree.TerminalNode;

public class GqlProbe {
    static final class Counter extends BaseErrorListener {
        int errors;
        final TreeSet<String> ambiguities = new TreeSet<>();

        @Override
        public void reportAmbiguity(Parser p, DFA dfa, int start, int stop, boolean exact, BitSet alts,
                                    ATNConfigSet configs) {
            BitSet a = alts != null ? alts : configs.getAlts();
            int rule = dfa.atnStartState.ruleIndex;
            // "outer": the decision between the rule's own alternatives; "inner": a sub-block.
            boolean outer = p.getATN().ruleToStartState[rule].transition(0).target == dfa.atnStartState;
            ambiguities.add("ambig:" + p.getRuleNames()[rule] + "/" + (outer ? "outer" : "inner" + dfa.decision)
                    + "/" + a.toString().replace(" ", ""));
        }
        @Override
        public void syntaxError(Recognizer<?, ?> r, Object o, int line, int col, String msg,
                                RecognitionException e) {
            errors++;
        }
    }

    public static void main(String[] args) throws Exception {
        String[] texts = Files.readString(Path.of(args[0])).split("\u0000", -1);
        boolean ambig = args.length > 1 && args[1].equals("ambig");
        StringBuilder out = new StringBuilder();
        for (String text : texts) {
            Counter c = new Counter();
            GQLLexer lexer = new GQLLexer(CharStreams.fromString(text));
            lexer.removeErrorListeners();
            lexer.addErrorListener(c);
            GQLParser parser = new GQLParser(new CommonTokenStream(lexer));
            parser.removeErrorListeners();
            parser.addErrorListener(c);
            parser.getInterpreter()
                    .setPredictionMode(ambig ? PredictionMode.LL_EXACT_AMBIG_DETECTION : PredictionMode.LL);
            ParseTree tree = parser.gqlProgram();
            if (c.errors > 0) {
                out.append("ERR\n");
                continue;
            }
            TreeSet<String> items = new TreeSet<>();
            walk(tree, parser, items);
            items.addAll(c.ambiguities);
            out.append("OK");
            for (String s : items) out.append(' ').append(s);
            out.append('\n');
        }
        System.out.print(out);
    }

    static void walk(ParseTree t, GQLParser p, TreeSet<String> items) {
        if (!(t instanceof ParserRuleContext ctx)) return;
        String rule = p.getRuleNames()[ctx.getRuleIndex()];
        items.add(rule);
        String cls = ctx.getClass().getSimpleName();
        String base = Character.toUpperCase(rule.charAt(0)) + rule.substring(1) + "Context";
        if (!cls.equals(base)) {
            String label = cls.substring(0, cls.length() - "Context".length());
            items.add(rule + "/#" + Character.toLowerCase(label.charAt(0)) + label.substring(1));
        } else {
            items.add(rule + "/" + ctx.getAltNumber());
        }
        for (int i = 0; i < ctx.getChildCount(); i++) {
            ParseTree ch = ctx.getChild(i);
            if (ch instanceof TerminalNode tn) {
                int type = tn.getSymbol().getType();
                if (type > 0) items.add(rule + ":" + p.getVocabulary().getSymbolicName(type));
            } else if (ch instanceof ParserRuleContext cc) {
                items.add(rule + ":" + p.getRuleNames()[cc.getRuleIndex()]);
                walk(cc, p, items);
            }
        }
    }
}
