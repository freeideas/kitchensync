package jLib;
import java.io.*;
import java.util.*;
import java.nio.file.*;

/**
 * A generic Result class that encapsulates success status, input, and output.
 *
 * @param <S> The type of the successful result.
 * @param <E> The type of the error result.
 */
public class Result<S,E> {
    private final boolean success;
    private final S okValue;
    private final E errValue;
    private boolean didCheckOK = false;
    private File logFile = null;

    public Result(boolean success, S okValue, E errValue) {
        if (success && errValue != null) throw new IllegalArgumentException("errValue should be null for success");
        if ((!success) && okValue != null) throw new IllegalArgumentException("okValue should be null for error");
        if (success && okValue == null) throw new IllegalArgumentException("okValue needed for success");
        if ((!success) && errValue == null) throw new IllegalArgumentException("errValue needed for error");
        this.success = success;
        this.okValue = okValue;
        this.errValue = errValue;
    }

    /**
     * Constructs a successful result.
     */
    public static <S,E> Result<S,E> ok(S okValue) {
        return new Result<>(true, okValue, null);
    }

    /**
     * Constructs an error result.
     */
    public static <S,E> Result<S,E> err(E errValue) {
        return new Result<>(false, null, errValue);
    }

    public boolean isOk() {
        didCheckOK = true;
        return success;
    }

    public S ok() {
        if (!didCheckOK) throw new IllegalStateException("Failed to check isOk");
        if (!success) throw new IllegalStateException("Cannot call ok() on an error result");
        return okValue;
    }

    public E err() {
        if (!didCheckOK) throw new IllegalStateException("Failed to check isOk");
        if (success) throw new IllegalStateException("Cannot call err() on a success result");
        return errValue;
    }

    public Result<S,E> setLogFile( File logFile ) {
        this.logFile = logFile;
        return this;
    }

    public Object toJsonable() {
        Map<String,Object> map = new LinkedHashMap<>();
        map.put("success", success);
        map.put("okValue", okValue);
        map.put("errValue", errValue);
        if (logFile!=null) {
            String filespec = logFile.toPath().relativize( Paths.get(".") ).toString();
            map.put("logFile",filespec);
            try { map.put( "logData", LibFile.file2string( logFile ) ); }
            catch (IOException e) { L0g.log(e); }
        }
        return map;
    }
    @SuppressWarnings({"unchecked","rawtypes"})
    public static <S,E> Result<S,E> fromJsonable( Object jsonable ) {
        Map<String,Object> map = (Map<String,Object>)jsonable;
        boolean success = (boolean)map.get("success");
        Object okValue = map.get("okValue");
        Object errValue = map.get("errValue");
        Result res = success ? Result.ok(okValue) : Result.err(errValue);
        String filespec = (String)map.get("logFile");
        if (filespec!=null) {
            res.logFile = new File(filespec);
            String logData = (String)map.get("logData");
            if (logData!=null) {
                res.logFile.delete();
                Lib.append2file(res.logFile, logData);
            }
        }
        return res;
    }

    public String toString() { return JsonEncoder.encode(this); }

    @SuppressWarnings("unused")
    private static boolean _TEST_() {
        { // test successful result
            Result<Integer, Exception> result = Result.ok(42);
            LibTest.asrt(result.isOk());
            LibTest.asrtEQ(42, result.ok());
            try {
                result.err();
                LibTest.asrt(false, "Should have thrown an exception");
            } catch (IllegalStateException expected) {}
        }
        { // test error result
            Result<String, Exception> result = Result.err(new Exception("Test error"));
            LibTest.asrt(!result.isOk());
            try {
                result.ok();
                LibTest.asrt(false, "Should have thrown an exception");
            } catch (IllegalStateException expected) {}
        }
        { // test exception on missing isOk check
            Result<String, Exception> result = Result.err(new Exception("Test error"));
            try {
                result.ok();
                LibTest.asrt(false, "Should have thrown an exception");
            } catch (IllegalStateException expected) {}
        }
        return true;
    }

    public static void main(String[] args) {
        LibTest.testClass();
    }
}
