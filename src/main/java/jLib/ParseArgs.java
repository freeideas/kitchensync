package jLib;
import java.util.*;
import java.io.File;
import java.lang.reflect.Array;


/**
 * USAGE EXAMPLE:
 * 
 * public static void main(String[] args) {
 *     // Create parser with command line args
 *     ParseArgs parser = new ParseArgs(args);
 *     
 *     // Configure the parser (optional)
 *     parser.setAppName("MyApp.exe")
 *           .setDescr("My Application - A tool for processing data")
 *           .setHelpFooter("\nFor more info, visit https://example.com\n");
 *     
 *     // Define and get arguments with types, defaults, and descriptions
 *     int port = parser.getInteger("port", 8080, "Server port number");
 *     boolean debug = parser.getBoolean("debug", false, "Enable debug mode");
 *     String config = parser.getString("config", null, "Configuration file path");
 *     float timeout = parser.getFloat("timeout", 30.0f, "Connection timeout in seconds");
 *     List<String> files = parser.getMulti("file", List.of(), "Input files to process");
 *     
 *     // Arguments can be passed in multiple formats:
 *     // java MyApp --port=9000 --debug -c=custom.conf --timeout=45.5 -f=input1.txt -f=input2.txt
 *     // java MyApp port=9000 debug=true config=custom.conf
 *     // java MyApp -p=9000 --debug  (abbreviations work: -p matches "port")
 *     // java MyApp @config.json  (loads args from JSON file)
 *     
 *     // Print help if requested
 *     if (parser.getBoolean("help", false, "Show this help message")) {
 *         System.out.println(parser.getHelp());
 *         System.exit(0);
 *     }
 *     
 *     // Use the parsed values
 *     System.out.println("Starting server on port " + port);
 *     if (debug) System.out.println("Debug mode enabled");
 * }
 * 
 * JSON FILE FORMAT (@filename.json):
 * - Object: {"port": "9000", "debug": "true"} → converted to --port=9000 --debug=true
 * - Array: ["--verbose", "--count=5"] → expanded as individual arguments
 * - String: "hello world" → used as a single argument value
 * 
 * ParseArgs - A flexible command-line argument parser with intelligent features:
 *
 * - Supports multiple argument formats: -arg=value, --arg=value, arg=value
 * - Boolean arguments can be specified as just --arg (implies true)
 * - Argument name abbreviation: -p=8080 matches "port" parameter
 * - Type-safe getters for String, Integer, Float, Boolean, and multi-value lists
 * - Automatic help generation with beautiful ASCII table formatting
 * - Smart text wrapping that optimizes column widths for readability
 * - Default values with clear documentation in help output
 * - Descriptive parameter documentation shown in aligned columns
 * - File references: @filename.json expands to the contents of the file
 *
 * Example: java MyApp --port=8080 --debug -f=config.txt --verbose
 */
public class ParseArgs {



    private String appName;
    private String descr;
    private String helpFooter;
    private String[] commandLine;
    private final Map< String, Map<String,Object> > name2usage = new LinkedHashMap<>();



    public ParseArgs( String[] commandLine ) {
        this.commandLine = expandFromFiles(commandLine);
    }



    public ParseArgs setAppName( String appName ) {
        this.appName = appName;
        return this;
    }
    public ParseArgs setDescr( String descr ) {
        this.descr = descr;
        return this;
    }
    public ParseArgs setHelpFooter( String helpFooter ) {
        this.helpFooter = helpFooter;
        return this;
    }



    /**
     * NOTE: Expands @filename references to JSON file contents in command line args.
     */
    private static String[] expandFromFiles( String[] args ) {
        if ( args==null ) return new String[0];
        List<String> result = new ArrayList<>();
        boolean hasFileRefs = false;
        for ( String arg: args ) {
            if ( arg==null || !arg.startsWith("@") ) {
                result.add(arg);
                continue;
            }
            hasFileRefs = true;
            String filePath = arg.substring(1);
            File file = new File(filePath);
            if ( !file.exists() ) throw new IllegalArgumentException( "File not found: "+filePath );
            if ( !file.canRead() ) throw new IllegalArgumentException( "Cannot read file: "+filePath );
            Object decoded;
            try { decoded = JsonDecoder.decode(file); }
            catch ( Exception e ) { throw new IllegalArgumentException( "Failed to parse JSON file: "+filePath, e ); }
            if ( decoded instanceof List<?> list ) {
                for ( Object item: list ) if ( item!=null ) result.add( item.toString() );
            } else if ( decoded instanceof Map<?,?> map ) {
                for ( Map.Entry<?,?> entry: map.entrySet() ) {
                    String key = entry.getKey().toString();
                    String value = entry.getValue()!=null ? entry.getValue().toString() : "";
                    result.add( "--"+key+"="+value );
                }
            } else {
                if ( decoded!=null ) result.add( decoded.toString() );
            }
        }
        if ( !hasFileRefs ) return args;
        return result.toArray( new String[0] );
    }



    public String getString( String keyName, String defaultValue, String argDesc ) {
        addUsage(keyName,"text",argDesc,defaultValue);
        String value = (String) findValue( keyName, "text" );
        if (value!=null) return value;
        return defaultValue;
    }
    public Integer getInteger( String keyName, Integer defaultValue, String argDesc ) {
        addUsage(keyName,"int",argDesc,defaultValue);
        Object value = findValue( keyName, "int" );
        if (value!=null) return (Integer)value;
        return defaultValue;
    }
    public Float getFloat( String keyName, Float defaultValue, String argDesc ) {
        addUsage(keyName,"float",argDesc,defaultValue);
        Object value = findValue( keyName, "float" );
        if (value!=null) return (Float)value;
        return defaultValue;
    }
    public Boolean getBoolean( String keyName, Boolean defaultValue, String argDesc ) {
        addUsage(keyName,"bool",argDesc,defaultValue);
        Object value = findValue( keyName, "bool" );
        if (value!=null) return (Boolean)value;
        return defaultValue;
    }
    public List<String> getMulti( String keyName, List<String> defaultValue, String argDesc ) {
        if (defaultValue==null) defaultValue = List.of();
        addUsage(keyName,"multi",argDesc,defaultValue);
        @SuppressWarnings("unchecked")
        List<String> value = (List<String>) findValue( keyName, "multi" );
        if (value!=null) return value;
        return Collections.unmodifiableList( new ArrayList<String>(defaultValue) );
    }



    public String getHelp() {
        if ( Lib.isEmpty(appName) ) appName =Lib.nvl( LibApp.getMainExeFile(), "(this program)" );
        final var LINE_LEN = 78;
        StringBuilder sb = new StringBuilder();
        // print command-line args
        sb.append('\n');
        sb.append(descr).append('\n');
        sb.append('\n');
        sb.append("ARGS:");
        for (var attr : name2usage.entrySet() ) {
            var argName = attr.getKey();
            var attrMap = attr.getValue();
            var argType = (String)attrMap.get("argType");
            var defaultValue = attrMap.get("defaultValue");
            sb.append(' ');
            if (defaultValue!=null) sb.append('[');
            sb.append( "-"+argName+"=("+argType+(argType.isBlank()?"":" ")+"value)" );
            if (defaultValue!=null) sb.append(']');
        }
        sb.append('\n');
        // print arg descriptions
        String[][] rows = new String[name2usage.size()+1][3];
        rows[0] = new String[]{"ARGNAME","TYPE","DESCRIPTION"};
        int entryIdx = 1;
        for (var attr : name2usage.entrySet() ) {
            var argName = attr.getKey();
            var attrMap = attr.getValue();
            var argType = (String)attrMap.get("argType");
            var argDesc = (String)attrMap.get("argDesc");
            var defaultValue = attrMap.get("defaultValue");
            if (argType==null) argType="";
            if (argDesc==null) argDesc="";
            if (defaultValue==null) defaultValue="";
            var argDescr = argDesc;
            if (! Lib.isEmpty(defaultValue) ) {
                argDescr = argDescr+" (default: "+defaultValue+")";
            }
            rows[entryIdx++] = new String[]{
                argName, argType, argDescr
            };
        }
        String[][][] wrappedRows = wrap( rows, LINE_LEN );
        StringBuilder sepLine = new StringBuilder();
        for (int rowIdx=0; rowIdx<wrappedRows.length; rowIdx++) {
            String[][] row = wrappedRows[rowIdx];
            // add separator line
            for (int colIdx=0; colIdx<row.length; colIdx++) {
                int colLen = row[colIdx][0].length();
                if (colIdx>0) {
                    sb.append('|');
                    if (rowIdx==0) sepLine.append('|');
                }
                sb.append( "=".repeat(colLen) );
                if (rowIdx==0) sepLine.append( "=".repeat(colLen) );
            }
            sb.append('\n');
            if (rowIdx==0) sepLine.append('\n');
            // add row
            for (int lineIdx=0; lineIdx<row[0].length; lineIdx++) {
                for (int colIdx=0; colIdx<row.length; colIdx++) {
                    String fieldLine = row[colIdx][lineIdx];
                    if (colIdx>0) sb.append('|');
                    sb.append(fieldLine);
                }
                sb.append('\n');
            }
        }
        sb.append(sepLine);
        sb.append(LibString.normalSpace("""
            * Args can look like "-arg=str" or "arg=true", or for boolean, just "--arg".
        """)+"\n");
        sb.append( "* Argument names can be abbreviated.\n" );
        sb.append( "* A json file can be referenced with e.g. \"@filename.json\".\n" );
        if (helpFooter!=null) sb.append(helpFooter);
        String s = sb.toString();
        return s;
    }



    private String[][][] wrap( String[][] lines, int lineLen ) {
        // Each element of the input is a row that consists of fields, which are strings.
        // This method will make each field into a String[] that is at most lineLen long.
        int rowCount = lines.length;
        if (rowCount==0) return new String[0][][];
        int colCount = lines[0].length;
        if (colCount==0) return new String[0][][];
        int lowestTotalFolds = Integer.MAX_VALUE;
        String[][][] bestLines = null;
        String[][][] linesCopy = new String[rowCount][colCount][];
        int[] bestWidths = null;
        // try every combination of column widths that add up to lineLen
        Iterable<int[]> it = () -> combinations(lineLen,colCount);
        OUTER_LOOP: for (int[] colWidths : it) {
            int totalFolds = 0;
            for (int rowIdx=0; rowIdx<lines.length; rowIdx++) {
                String[] row = lines[rowIdx];
                for (int colIdx=0; colIdx<row.length; colIdx++) {
                    String field = row[colIdx];
                    String[] wrapped = LibString.wrapText(field,colWidths[colIdx],false);
                    for (String word : wrapped) {
                        if ( word.length() > colWidths[colIdx] ) {
                            continue OUTER_LOOP; // doesn't fit
                        }
                    }
                    linesCopy[rowIdx][colIdx] = wrapped;
                    totalFolds += wrapped.length;
                }
            }
            if (totalFolds<lowestTotalFolds) {
                // we've found a better solution
                lowestTotalFolds = totalFolds;
                bestLines = (String[][][]) deepCopyArray(linesCopy);
                bestWidths = (int[]) deepCopyArray(colWidths);
            }
        }
        // center each field horizontally and vertically
        for (int rowIdx=0; rowIdx<bestLines.length; rowIdx++) {
            String[][] row = bestLines[rowIdx];
            int maxLines = 0;
            for (int colIdx=0; colIdx<row.length; colIdx++) {
                String[] fieldLines = row[colIdx];
                int lineCount = fieldLines.length;
                if (lineCount>maxLines) maxLines=lineCount;
            }
            for (int colIdx=0; colIdx<row.length; colIdx++) {
                String[] fieldLines = row[colIdx];
                fieldLines = centerHV( fieldLines, bestWidths[colIdx], maxLines );
                row[colIdx] = fieldLines;
            }
        }
        return bestLines;
    }

    private static String[] centerHV( String[] fieldLines, int width, int lineCount ) {
        List<String> lines = new ArrayList<>();
        lines.addAll( Arrays.asList(fieldLines) );
        boolean doCenterH = lines.size() <= 1;
        while ( lines.size() < lineCount ) {
            lines.add("");
            if ( lines.size() < lineCount ) lines.add(0,"");
        }
        for (int lineIdx=0,len=lines.size(); lineIdx<len; lineIdx++) {
            String line = lines.get(lineIdx);
            line = doCenterH ? LibString.centerPad(line,width) : LibString.rpad(line,width," ");
            lines.set(lineIdx,line);
        }
        return lines.toArray(new String[0]);
    }

    private static Iterator<int[]> combinations( int sum, int len ) {
        /**
        * Yields every combination of integers that add up to a given sum.
        **/
        if (len==0) return new ArrayList<int[]>().iterator();
        if (len==1) return Arrays.asList(new int[]{sum}).iterator();
        return new Iterator<int[]>() {
            int firstIdx = -1;
            Iterator<int[]> curIt = null;
            @Override
            public boolean hasNext() {
                if (curIt==null) nextIt();
                if (curIt==null) return false;
                if (curIt.hasNext()) return true;
                curIt = null;
                return hasNext();
            }
            @Override
            public int[] next() {
                if (! hasNext() ) throw new NoSuchElementException();
                int[] tail = curIt.next();
                int[] result = new int[ tail.length+1 ];
                result[0] = firstIdx;
                System.arraycopy( tail,0, result,1, tail.length );
                return result;
            }
            private void nextIt() {
                firstIdx++;
                curIt = null;
                if (firstIdx>sum) return;
                curIt = combinations( sum-firstIdx, len-1 );
                if (! curIt.hasNext() ) nextIt();
            }
        };
    }
    @SuppressWarnings("unused")
    private static boolean combinations_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        List<int[]> results = new ArrayList<>();
        for ( int[] comb: Lib.iterable( combinations(5,3) ) ) results.add(comb);
        LibTest.asrtEQ( 21, results.size() );
        for ( int[] comb: results ) {
            LibTest.asrtEQ( 3, comb.length );
            LibTest.asrtEQ( 5, comb[0]+comb[1]+comb[2] );
        }
        return true;
    }



    /**
    * Type is one of: int, float, string, boolean, multi
    **/
    private ParseArgs addUsage(
        String argName, String argType, String argDesc, Object defaultValue
    ) {
        if ( Lib.isEmpty(argName) ) throw new IllegalArgumentException( "argName is required" );
        argName = argName.trim().toLowerCase(); // Normalize case to match findValue
        if ( Lib.isEmpty(argType) ) argType = "text";
        if ( Lib.isEmpty(argDesc) ) argDesc = "";
        if ( defaultValue==null ) defaultValue = argType.equals("multi") ? List.of() : null;
        Map<String,Object> attrMap = new LinkedHashMap<>();
        attrMap.put("argType",argType);
        attrMap.put("argDesc",argDesc);
        attrMap.put("defaultValue",defaultValue);
        name2usage.put(argName,attrMap);
        return this;
    }



    /**
    * Collects all values whose keys are prefixes of keyName.
    * Searches command line for "-keyName=value" or "--keyName=value" or "keyname=value".
    * If type is "multi", then the value is a list of strings, otherwise returns a String, Integer, Float, or Boolean.
    **/
    private Object findValue( String keyName, String type ) {
        if ( Lib.isEmpty(keyName) ) return null;
        keyName = keyName.trim().toLowerCase();
        if ( Lib.isEmpty(type) ) type = "text";
        type = type.trim().toLowerCase();
        List<Object> values = new ArrayList<>();
        for ( String argument: commandLine ) {
            String arg = argument.trim();
            while ( arg.startsWith("-") ) arg = arg.substring(1);
            String[] parts = arg.split( "=", 2 );
            if ( parts.length < 1 ) continue;
            String argName = parts[0];
            if (! keyName.startsWith( argName.trim().toLowerCase() ) ) continue;
            String argValue = parts.length > 1 ? parts[1] : "";
            if ( Lib.isEmpty(argValue) && type.equals("bool") ) argValue = "true";
            if ( Lib.isEmpty(argValue) ) continue;
            values.add(argValue);
        }
        Object defaultValue = name2usage.get(keyName).get("defaultValue");
        if ( values.isEmpty() ) return defaultValue;
        switch ( type ) {
            case "int": return Integer.parseInt( values.get(0).toString() );
            case "float": return Float.parseFloat( values.get(0).toString() );
            case "bool": return Lib.isTrue( values.get(0) );
            case "multi": return Collections.unmodifiableList(values);
            default: return values.get(0);
        }
    }



    @SuppressWarnings("unused")
    private static boolean basic_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        {
            String[] args = new String[]{ "-a=10", "-b", "--ccc=a", "-dd=2.3", "e=A", "-ee=B", "--EEE=C" };
            ParseArgs p = new ParseArgs(args);
            p.setAppName("Test.exe");
            p.setDescr("This tests the arg parser");
            p.setHelpFooter("This is the footer");
            int aaa = p.getInteger( "aaa",1,"How many?" );
            boolean bbb = p.getBoolean( "bbb",false,"Is it on?" );
            String ccc = p.getString( "ccc","default CCC","What is it?" );
            float ddd = p.getFloat( "ddd",0.0f,"How much?" );
            List<String> eee = p.getMulti( "eee",List.of("e1","e2"),"What are they?" );
            String help = p.getHelp();
            System.out.println(help);
            LibTest.asrtEQ( aaa, 10 );
            LibTest.asrtEQ( bbb, true );
            LibTest.asrtEQ( ccc, "a" );
            LibTest.asrtEQ( ddd, 2.3f );
        }
        return true;
    }
    @SuppressWarnings("unused")
    private static boolean expandFromFiles_TEST_( boolean findLineNumber ) throws Exception {
        if ( findLineNumber ) throw new RuntimeException();
        try ( TmpDir tmpDir = new TmpDir() ) {
            File mapFile = new File( tmpDir.dir, "map.json" );
            File listFile = new File( tmpDir.dir, "list.json" );
            File stringFile = new File( tmpDir.dir, "string.json" );
            LibFile.string2file( "{\"port\":\"8080\",\"debug\":\"true\"}", mapFile, false );
            LibFile.string2file( "[\"--verbose\",\"--count=5\"]", listFile, false );
            LibFile.string2file( "\"hello world\"", stringFile, false );
            String[] args1 = { "--start", "@"+mapFile.getPath(), "--end" };
            String[] result1 = expandFromFiles(args1);
            LibTest.asrtEQ( result1.length, 4 );
            LibTest.asrtEQ( result1[0], "--start" );
            LibTest.asrtEQ( result1[1], "--port=8080" );
            LibTest.asrtEQ( result1[2], "--debug=true" );
            LibTest.asrtEQ( result1[3], "--end" );
            String[] args2 = { "--begin", "@"+listFile.getPath(), "--finish" };
            String[] result2 = expandFromFiles(args2);
            LibTest.asrtEQ( result2.length, 4 );
            LibTest.asrtEQ( result2[0], "--begin" );
            LibTest.asrtEQ( result2[1], "--verbose" );
            LibTest.asrtEQ( result2[2], "--count=5" );
            LibTest.asrtEQ( result2[3], "--finish" );
            String[] args3 = { "--prefix", "@"+stringFile.getPath(), "--suffix" };
            String[] result3 = expandFromFiles(args3);
            LibTest.asrtEQ( result3.length, 3 );
            LibTest.asrtEQ( result3[0], "--prefix" );
            LibTest.asrtEQ( result3[1], "hello world" );
            LibTest.asrtEQ( result3[2], "--suffix" );
            String[] args4 = { "--no-files", "--here" };
            String[] result4 = expandFromFiles(args4);
            LibTest.asrt( result4==args4 );
        }
        return true;
    }



    private static Object deepCopyArray( Object arr ) {
        int len = Array.getLength(arr);
        Object result = Array.newInstance(arr.getClass().getComponentType(), len);
        for ( int i=0; i<len; i++ ) {
            Object o = Array.get(arr, i);
            if ( o!=null && o.getClass().isArray() ) o = deepCopyArray(o);
            Array.set(result, i, o);
        }
        return result;
    }



    public static void main( String[] args ) throws Exception { LibTest.testClass(); }
}
