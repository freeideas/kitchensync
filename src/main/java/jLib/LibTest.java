package jLib;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;


public class LibTest {

    /**
     * Assertion method that throws AssertionError if condition is false
     */
    public static Object asrt( Object o, Object... msgs ) {
        if ( Lib.isTrue(o) ) return o;
        String msg = null;
        for ( Object m:msgs ) {
            if ( msg==null ) msg = "" + m;
            //System.err.println(m);
        }
        if ( msg==null ) msg = "" + o;
        throw new AssertionError("asrt failed: " + msg);
    }
    @SuppressWarnings({"unused", "EmptyCatchBlock"})
    private static boolean asrt_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        try {
            asrt(true, "This should not fail");
            asrt(1>0, "This should not fail");
            try {
                asrt(false);
                return false;
            } catch ( AssertionError e ) {}
            try {
                asrt(1==2);
                return false;
            } catch ( AssertionError e ) {}
            return true;
        } catch ( Throwable t ) {
            t.printStackTrace();
            return false;
        }
    }


    /**
     * Assertion method that checks equality and throws AssertionError if not equal
     */
    public static boolean asrtEQ( Object expected, Object actual, Object... msgs ) {
        if ( Lib.isEqual(expected,actual) ) return true;
        String msg = "expected=" + expected + "; actual=" + actual;
        //System.err.println(msg);
        if ( msgs.length==0 ) msgs = new String[]{msg};
        asrt(false, msgs);
        return false;
    }
    @SuppressWarnings({"unused", "EmptyCatchBlock"})
    private static boolean asrtEQ_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        try {
            asrtEQ(1, 1, "This should not fail");
            asrtEQ("a", "a", "This should not fail");
            asrtEQ(null, null, "This should not fail");
            try {
                asrtEQ(1,2);
                return false;
            } catch ( AssertionError e ) {}
            try {
                asrtEQ("a","b");
                return false;
            } catch ( AssertionError e ) {}
            try {
                asrtEQ(null,"b");
                return false;
            } catch ( AssertionError e ) {}
            asrtEQ(Arrays.asList("a","b","c"), Arrays.asList("a","b","c"), "This should not fail");
            try {
                asrtEQ(Arrays.asList("a","b","c"), Arrays.asList("a","b"));
                return false;
            } catch ( AssertionError e ) {}
            return true;
        } catch ( Throwable t ) {
            t.printStackTrace();
            return false;
        }
    }


    /**
     * Tests all methods in a class that end with _TEST_
     */
    public static boolean testClass() {
        Class<?> clas = Lib.getCallingClass();
        return testClass(clas);
    }
    public static boolean testClass( Class<?> claz ) {
        Class<?> clas = claz==null ? Lib.getCallingClass() : claz;
        LibApp.archiveLogFiles();
        System.out.println("Testing class: " + clas.getName());
        class MethodInfo implements Comparable<MethodInfo> {
            public final Method method;
            public int methodLineNumber = Integer.MAX_VALUE;
            public StackTraceElement[] errLoc = null;
            public String errMsg = null;
            MethodInfo( Method m ) { this.method = m; }
            public void setMethodLineNumber( Throwable t ) {
                StackTraceElement[] methodLocation = t.getStackTrace();
                String methNam = method.getName();
                for (StackTraceElement ste : methodLocation) {
                    if ( ste.getClassName().equals(clas.getName()) && ste.getMethodName().equals(methNam) ) {
                        methodLineNumber = ste.getLineNumber();
                        return;
                    }
                }
            }
            public void setErrorTrace( Throwable t ) {
                StackTraceElement[] methodLocation = t.getStackTrace();
                // filter out anything not from clas
                errLoc = Arrays.stream(methodLocation).filter(
                    ste -> ste.getClassName().equals(clas.getName())
                ).toArray(StackTraceElement[]::new);
            }
            @Override
            public int compareTo(MethodInfo other) {
                int lineCompare = Integer.compare( this.methodLineNumber, other.methodLineNumber );
                if (lineCompare != 0) return lineCompare;
                return this.method.getName().compareTo(other.method.getName());
            }
            @Override
            public String toString() {
                String s = (
                    clas.getSimpleName() + "." + method.getName() +
                    ( methodLineNumber==Integer.MAX_VALUE ? "" : ":"+ methodLineNumber )
                );
                if (errMsg!=null) s += "\n   "+errMsg;
                if (errLoc!=null) {
                    for (StackTraceElement ste : errLoc) s += "\n   "+ste.toString();
                }
                return s;
            }
        }
        // Collect and sort methods
        List<MethodInfo> testMethods = new ArrayList<>();
        for (Method m : clas.getDeclaredMethods()) {
            String methNam = m.getName();
            if (!methNam.endsWith("_TEST_")) continue;
            if (!Modifier.isStatic(m.getModifiers())) continue;
            MethodInfo mi = new MethodInfo(m);
            testMethods.add(mi);
            try {
                m.setAccessible(true);
                if (m.getParameterCount()==1 && m.getParameterTypes()[0] == boolean.class) {
                    m.invoke(null, true);  // finding line number this time
                }
            } catch ( Throwable t ) {
                if ( t instanceof InvocationTargetException ite ) t = ite.getTargetException();
                mi.setMethodLineNumber(t);
            }
        }
        Collections.sort(testMethods);
        // Run tests in order of line number
        List<MethodInfo> failedMethods = new ArrayList<>();
        for (MethodInfo methodInfo : testMethods) {
            Method m = methodInfo.method;
            System.out.println( "Running test: " + methodInfo.toString() );
            try {
                m.setAccessible(true);
                Object res;
                if (!java.lang.reflect.Modifier.isStatic(m.getModifiers())) {
                    throw new RuntimeException( "not static" );
                }
                if (m.getParameterCount()==1 && m.getParameterTypes()[0] == boolean.class) {
                    res = m.invoke(null, false);  // not finding line number this time
                } else res = m.invoke(null);
                if ( Boolean.FALSE.equals(res) ) {
                    String failMsg = "Returned false: "+ methodInfo;
                    System.out.println(failMsg);
                    failedMethods.add(methodInfo);
                }
            } catch (Throwable t) {
                failedMethods.add(methodInfo);
                if ( t instanceof InvocationTargetException ite ) t = ite.getTargetException();
                String failMsg = "Fail in method "+methodInfo+": "+LibString.dblQuot( t.getMessage() );
                System.out.println(failMsg);
                methodInfo.setErrorTrace(t);
                methodInfo.errMsg = t.getMessage();
            }
        }
        if ( failedMethods.isEmpty() ) {
            System.out.println( "All tests PASS in class "+clas.getName() );
            return true;
        }
        System.out.println( "Failed tests in class "+clas.getName()+":" );
        for ( MethodInfo mi : failedMethods ) System.out.println(mi);
        return false;
    }


    /**
     * Tests all classes in the project by finding all .java files and running their tests
     */
    public static boolean testAllClasses() {
        List<java.io.File> javaFiles = new ArrayList<>();
        findJavaFiles(new java.io.File("."), javaFiles);
        
        // Sort files alphabetically by path
        javaFiles.sort((a, b) -> a.getPath().compareTo(b.getPath()));
        
        List<String> failedClasses = new ArrayList<>();
        int totalClasses = 0;
        int testedClasses = 0;
        
        for (java.io.File javaFile : javaFiles) {
            String className = fileToClassName(javaFile);
            if (className == null) continue;
            
            
            totalClasses++;
            try {
                Class<?> claz = Class.forName(className);
                System.out.println("\n========== Testing " + className + " ==========");
                boolean passed = testClass(claz);
                testedClasses++;
                if (!passed) failedClasses.add(className);
            } catch (ClassNotFoundException e) {
                // Skip files that don't have corresponding classes
            } catch (Throwable t) {
                System.out.println("Error testing " + className + ": " + t.getMessage());
                failedClasses.add(className);
            }
        }
        
        System.out.println("\n========== SUMMARY ==========");
        System.out.println("Total classes found: " + totalClasses);
        System.out.println("Classes tested: " + testedClasses);
        System.out.println("Classes passed: " + (testedClasses - failedClasses.size()));
        System.out.println("Classes failed: " + failedClasses.size());
        
        if (!failedClasses.isEmpty()) {
            System.out.println("\nFailed classes:");
            for (String className : failedClasses) System.out.println("  " + className);
            return false;
        }
        return true;
    }
    
    private static void findJavaFiles(java.io.File dir, List<java.io.File> javaFiles) {
        if (!dir.exists() || !dir.isDirectory()) return;
        java.io.File[] files = dir.listFiles();
        if (files == null) return;
        
        for (java.io.File file : files) {
            if (file.isDirectory()) {
                findJavaFiles(file, javaFiles);
            } else if (file.getName().endsWith(".java")) {
                javaFiles.add(file);
            }
        }
    }
    
    private static String fileToClassName(java.io.File javaFile) {
        String path = javaFile.getAbsolutePath();

        // Normalize path separators to forward slashes for consistent matching
        path = path.replace('\\', '/');

        // Find the source root (src/main/java or src/test/java or just src)
        String[] sourceRoots = {"/src/main/java/", "/src/test/java/", "/src/"};
        String className = null;

        for (String sourceRoot : sourceRoots) {
            int idx = path.indexOf(sourceRoot);
            if (idx != -1) {
                className = path.substring(idx + sourceRoot.length());
                break;
            }
        }
        
        if (className == null) return null;
        
        // Remove .java extension and convert path separators to dots
        if (className.endsWith(".java")) {
            className = className.substring(0, className.length() - 5);
        }
        className = className.replace(java.io.File.separatorChar, '.');
        
        return className;
    }


    public static void main( String[] args ) { 
        boolean passed = testAllClasses();
        System.exit(passed ? 0 : 1);
    }
}