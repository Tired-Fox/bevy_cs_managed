using System;
using System.Buffers;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Text;
using System.Text.Json;

class Utils
{
    public static string ReadUtf8Z(IntPtr p)
    {
        if (p == IntPtr.Zero) return string.Empty;
        return Marshal.PtrToStringUTF8(p)!;
    }
}

public class RuntimeAssembly
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetClassDelegate(Assembly assembly, IntPtr typeNameUtf8Z, out IntPtr result); // returns type handle

    public static void GetClass(Assembly assembly, IntPtr typeNameUtf8Z, out IntPtr result)
    {
        var tn = Utils.ReadUtf8Z(typeNameUtf8Z);
        var t = ResolveTypeInAsm(assembly, tn) ?? throw new TypeLoadException($"Type not found: {tn}");
        if (t == null) {
            result = IntPtr.Zero;
        } else {
            result = Host.Pin(t);
        }
    }

    static Type? ResolveTypeInAsm(Assembly assembly, string fullOrShort)
    {
        var t = Type.GetType(fullOrShort, throwOnError: false, ignoreCase: false);
        if (t != null) return t;

        t = assembly.GetType(fullOrShort, throwOnError: false, ignoreCase: false);
        if (t != null) return t;

        return assembly.GetTypes().FirstOrDefault(x => string.Equals(x.FullName, fullOrShort, StringComparison.Ordinal));
    }

    public static Type? ResolveTypeInContext(Scope scope, string fullOrShort) => null;
}

public sealed class RuntimeClass
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void NewDelegate(IntPtr klass, out IntPtr result);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void IsAssignableFromDelegate(IntPtr baseKlass, IntPtr targetKlass, out int result);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetMethodDelegate(IntPtr klass, IntPtr nameUtf8Z, int argCount, out IntPtr result);

    public static void IsAssignableFrom(IntPtr baseKlass, IntPtr targetKlass, out int result)
    {
        var baseType = Host.Ref<Type>(baseKlass);
        var targetType = Host.Ref<Type>(targetKlass);
        result = baseType.IsAssignableFrom(targetType) ? 1 : 0;
    }

    public static void New(IntPtr klass, out IntPtr result)
    {
        var t = Host.Ref<Type>(klass);
        var obj = Activator.CreateInstance(t) ?? throw new MissingMethodException($"No default ctor for {t.FullName}");
        result = Host.Pin(obj);
    }

    public static void GetMethod(IntPtr klass, IntPtr name, int argCount, out IntPtr result)
    {
        var t = Host.Ref<Type>(klass);
        var methodName = Utils.ReadUtf8Z(name);

        var flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance;
        var cand = t.GetMethods(flags).Where(m => m.Name == methodName && m.GetParameters().Length == argCount).FirstOrDefault<MethodInfo>();

        result = cand == null ? IntPtr.Zero : Host.Pin(cand);
    }
}

public sealed class RuntimeMethod
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void RuntimeInvokeDelegate(IntPtr method, void* instance, void** args);
    public unsafe static void RuntimeInvoke(IntPtr method, void* instancePtr, void** argv)
    {
        Console.WriteLine("Runtime invoking method");

        var m = Host.Ref<MethodInfo>(method);

        Console.WriteLine("  - Get params and arg length");
        var parameters = m.GetParameters();
        var argc = parameters.Length;

        object? instance = null;
        if (instancePtr != null)
        {
            Console.WriteLine("  - Unbox instance");
            instance = Host.Ref<object>((IntPtr)instancePtr);
        }

        Console.WriteLine($"  - Unbox args [{argc}]");
        var args = new object?[argc];
        for (var i = 0; i < argc; i++)
        {
            Console.WriteLine($"    - {parameters[i].ParameterType.FullName}");
            if (parameters[i].ParameterType.IsValueType) {
                Console.WriteLine($"    - value null:{argv[i]==null}");
                args[i] = ReadValueAsObject(argv[i], parameters[i].ParameterType);
                Console.WriteLine("    - complete");
            } else if (parameters[i].ParameterType == typeof(string)) {
                Console.WriteLine("    - string");
                args[i] = Marshal.PtrToStringUTF8((IntPtr)argv[i]);
                Console.WriteLine("    - complete");
            } else {
                Console.WriteLine("    - GCHandle");
                args[i] = GCHandle.FromIntPtr((IntPtr)argv[i]).Target;
                Console.WriteLine("    - complete");
            }
        }

        Console.WriteLine("  - Invoke");
        m.Invoke(instance, args);
    }

    private static unsafe object? ReadValueAsObject(void* p, Type t)
    {
        var elem = t.IsByRef ? t.GetElementType() : t;

        if (elem == typeof(IntPtr) || elem == typeof(nint))
        {
            nint val = p == null ? default : Unsafe.Read<nint>(p);
            return (IntPtr)val;
        }

        // Fast path for common primitives
        if (elem == typeof(int)) return Unsafe.Read<int>(p);
        if (elem == typeof(uint))   return Unsafe.Read<uint>(p);
        if (elem == typeof(long))   return Unsafe.Read<long>(p);
        if (elem == typeof(ulong))  return Unsafe.Read<ulong>(p);
        if (elem == typeof(short))  return Unsafe.Read<short>(p);
        if (elem == typeof(ushort)) return Unsafe.Read<ushort>(p);
        if (elem == typeof(byte))   return Unsafe.Read<byte>(p);
        if (elem == typeof(sbyte))  return Unsafe.Read<sbyte>(p);
        if (elem == typeof(bool))   return Unsafe.Read<byte>(p) != 0;       // define size!
        if (elem == typeof(float))  return Unsafe.Read<float>(p);
        if (elem == typeof(double)) return Unsafe.Read<double>(p);
        if (elem.IsEnum)
        {
            var u = Enum.GetUnderlyingType(t);
            object raw = ReadValueAsObject(p, u);
            return Enum.ToObject(t, Convert.ChangeType(raw, u));
        }

        // Blittable structs → Marshal.PtrToStructure (boxed)
        // (You can replace with Unsafe.Read<T> if you constrain to blittable T known at compile time.)
        return Marshal.PtrToStructure((IntPtr)p, t)!;
    }
}

public sealed class Scope : AssemblyLoadContext
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void LoadFromPathDelegate(IntPtr scopeId, IntPtr pathUtf8Z, out Assembly result);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void LoadFromBytesDelegate(IntPtr scopeId, IntPtr bytes, int length, out Assembly result);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void UnloadDelegate(IntPtr scopeId);

    public readonly string BaseDir;

    public Scope(string baseDir) : base(isCollectible: true)
    {
        BaseDir = baseDir;
    }

    public static void LoadFromPath(IntPtr scope, IntPtr path, out Assembly result)
    {
        var self = Host.Ref<Scope>(scope);
        var p = Path.Combine(self.BaseDir, Utils.ReadUtf8Z(path));
        try
        {
            result = self.LoadFromAssemblyPath(p);
        }
        catch
        {
            result = null;
        }
    }

    public static void LoadFromBytes(IntPtr scope, IntPtr bytes, int length, out Assembly result)
    {
        unsafe
        {
            var self = Host.Ref<Scope>(scope);
            var span = new ReadOnlySpan<byte>((void*)bytes, length);
            using var ms = new MemoryStream(span.ToArray());
            try
            {
                result = self.LoadFromStream(ms);
            }
            catch
            {
                result = null;
            }
        }
    }

    public static void Unload(IntPtr scope)
    {
        var handle = GCHandle.FromIntPtr(scope);
        var target = (Scope)handle.Target;

        target.Unload();
        handle.Free();

        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
    }
}

public static class Host
{
    // ---------- Delegates (unmanaged stubs will match these) ----------
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void CreateScopeDelegate(IntPtr baseDir, out IntPtr result);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void DestroyDelegate(IntPtr handle);  // frees any object/type/assembly handle
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void FreeDelegate(IntPtr ptr);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void PingDelegate(out uint result);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate IntPtr CallMethodJsonDelegate(IntPtr objectHandle, IntPtr methodNameUtf8Z, IntPtr jsonArgsUtf8Z, out int outLen);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate IntPtr CallStaticJsonDelegate(IntPtr scopeId, IntPtr typeNameUtf8Z, IntPtr methodNameUtf8Z, IntPtr jsonArgsUtf8Z, out int outLen);

    // ---------- State & handles ----------
    static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
        AllowTrailingCommas = true
    };

    // ---------- helpers ----------
    static unsafe string ReadUtf8Z(IntPtr p)
    {
        if (p == IntPtr.Zero) return string.Empty;
        return Marshal.PtrToStringUTF8(p)!;
    }

    static IntPtr AllocUtf8(string s, out int len)
    {
        // allocate CoTaskMem UTF-8 (host must call FreeMemory)
        var bytes = Encoding.UTF8.GetBytes(s);
        len = bytes.Length;
        var dst = Marshal.AllocCoTaskMem(len + 1);
        Marshal.Copy(bytes, 0, dst, len);
        Marshal.WriteByte(dst, len, 0);
        return dst;
    }

    public static IntPtr Pin(object obj, bool pinned = false) => GCHandle.ToIntPtr(GCHandle.Alloc(obj, pinned ? GCHandleType.Pinned : GCHandleType.Normal));
    public static T Ref<T>(IntPtr target) => (T)GCHandle.FromIntPtr(target).Target;
    public static void Unpin(IntPtr id) => GCHandle.FromIntPtr(id).Free();

    static object? InvokeWithJson(object? targetOrNull, Type declaringType, string methodName, string jsonArgs)
    {
        // Interpret args JSON as array: [ ... ]
        object[] args = Array.Empty<object>();
        if (!string.IsNullOrWhiteSpace(jsonArgs))
        {
            using var doc = JsonDocument.Parse(jsonArgs);
            if (doc.RootElement.ValueKind == JsonValueKind.Array)
                args = doc.RootElement.EnumerateArray().Select(e => (object)e.Clone()).ToArray();
            else if (doc.RootElement.ValueKind != JsonValueKind.Undefined && doc.RootElement.ValueKind != JsonValueKind.Null)
                args = new object[] { doc.RootElement.Clone() };
        }

        // Find method candidates: instance or static depending on targetOrNull
        var flags = BindingFlags.Public | BindingFlags.NonPublic | (targetOrNull is null ? BindingFlags.Static : BindingFlags.Instance);
        var cands = declaringType.GetMethods(flags).Where(m => m.Name == methodName);

        foreach (var m in cands)
        {
            var ps = m.GetParameters();
            if (ps.Length != args.Length) continue;

            var callArgs = new object?[ps.Length];
            bool ok = true;

            for (int i = 0; i < ps.Length && ok; i++)
            {
                var ptype = ps[i].ParameterType;

                // If we kept JsonElement clones, materialize to the exact parameter type
                if (args[i] is JsonElement je)
                {
                    try
                    {
                        // deserialize directly to the param type
                        callArgs[i] = JsonSerializer.Deserialize(je.GetRawText(), ptype, JsonOpts);
                    }
                    catch
                    {
                        ok = false;
                    }
                }
                else
                {
                    // last-ditch convertible
                    try { callArgs[i] = Convert.ChangeType(args[i], ptype); }
                    catch { ok = false; }
                }
            }

            if (!ok) continue;

            return m.Invoke(targetOrNull, callArgs);
        }

        throw new MissingMethodException($"No compatible overload for {declaringType.FullName}.{methodName} with {args.Length} args.");
    }

    // ---------- Exported (via managed delegates) ----------
    public static void Ping(out uint result) => result = 1;

    public static void CreateScope(IntPtr baseDir, out IntPtr result)
    {
        // Use current assembly directory for probing by default
        var dir = baseDir == IntPtr.Zero ? Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location)! : ReadUtf8Z(baseDir);
        result = Pin(new Scope(dir));
    }

    public static void Destroy(IntPtr handle) => Unpin(handle);

    public static void Free(IntPtr ptr)
    {
        if (ptr != IntPtr.Zero) Marshal.FreeCoTaskMem(ptr);
    }

    public static IntPtr CallMethodJson(IntPtr objectHandle, IntPtr methodNameUtf8Z, IntPtr jsonArgsUtf8Z, out int outLen)
    {
        var obj = Ref<object>(objectHandle);
        var method = ReadUtf8Z(methodNameUtf8Z);
        var json = ReadUtf8Z(jsonArgsUtf8Z);
        var result = InvokeWithJson(obj, obj.GetType(), method, json);
        var payload = JsonSerializer.Serialize(result, JsonOpts);
        return AllocUtf8(payload, out outLen);
    }

    public static IntPtr CallStaticJson(IntPtr scopeHandle, IntPtr typeNameUtf8Z, IntPtr methodNameUtf8Z, IntPtr jsonArgsUtf8Z, out int outLen)
    {
        var scope = Ref<Scope>(scopeHandle);
        var tn = ReadUtf8Z(typeNameUtf8Z);
        var t = RuntimeAssembly.ResolveTypeInContext(scope, tn) ?? throw new TypeLoadException($"Type not found: {tn}");
        var method = ReadUtf8Z(methodNameUtf8Z);
        var json = ReadUtf8Z(jsonArgsUtf8Z);
        var result = InvokeWithJson(null, t, method, json);
        var payload = JsonSerializer.Serialize(result, JsonOpts);
        return AllocUtf8(payload, out outLen);
    }
}
