// Prints the ANTLR parse tree (LL mode, as the oracle) of each line of stdin.
import java.io.*;
import org.antlr.v4.runtime.*;
import org.antlr.v4.runtime.atn.PredictionMode;
public class TreeDump {
    public static void main(String[] a) throws Exception {
        BufferedReader r = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
        for (String line; (line = r.readLine()) != null; ) {
            GQLParser p = new GQLParser(new CommonTokenStream(new GQLLexer(CharStreams.fromString(line))));
            p.getInterpreter().setPredictionMode(PredictionMode.LL);
            System.out.println(line + "\n  " + p.gqlProgram().toStringTree(p));
        }
    }
}
