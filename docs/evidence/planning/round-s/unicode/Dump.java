import org.antlr.v4.runtime.*;
// Prints "cp S" or "cp C" for every code point the lexer matches as ID_Start (S) or ID_Continue only (C).
public class Dump {
  public static void main(String[] a) {
    StringBuilder out = new StringBuilder();
    for (int cp = 0; cp <= 0x10FFFF; cp++) {
      if (cp >= 0xD800 && cp <= 0xDFFF) continue;
      U lx = new U(CharStreams.fromString(new String(Character.toChars(cp))));
      lx.removeErrorListeners();
      Token t = lx.nextToken();
      if (t.getType() == U.S) out.append(cp).append(" S\n");
      else if (t.getType() == U.C) out.append(cp).append(" C\n");
    }
    System.out.print(out);
  }
}
