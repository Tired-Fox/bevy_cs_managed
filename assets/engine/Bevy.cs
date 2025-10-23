using System;
namespace Engine;

public struct Vector3 {
    public float x;
    public float y;
    public float z;

    public static Vector3 Zero = new Vector3(0, 0, 0);

    public Vector3(float x, float y, float z)
    {
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public Vector3(float value)
    {
        x = value;
        y = value;
        z = value;
    }

    /**
     * <summary>Faster for magnitude comparison as it doesn't calculate the square root</summary>
     */
    public float SqrMagnitude() => (x*x)+(y*y)+(z*z);
    public float Magnitude() => (float)Math.Sqrt((x*x)+(y*y)+(z*z));
    public Vector3 Normalize() => this / Magnitude();

    public static float Dot(Vector3 left, Vector3 right) => (left.x * right.x) + (left.y * right.y) + (left.z * right.z);
    public static Vector3 Cross(Vector3 left, Vector3 right) => new Vector3(
            (left.y * right.z) - (left.z * right.y),
            (left.z * right.x) - (left.x * right.z),
            (left.x * right.y) - (left.y * right.x)
    );

    public override string ToString() => $"({x}, {y}, {z})";

    public static explicit operator Vector3(float value) => new Vector3{ x = value, y = value, z = value };

    public static Vector3 operator +(Vector3 left, Vector3 right) => new Vector3(left.x + right.x, left.y + right.y, left.z + right.z);
    public static Vector3 operator -(Vector3 left, Vector3 right) => new Vector3(left.x - right.x, left.y - right.y, left.z - right.z);
    public static Vector3 operator *(Vector3 left, Vector3 right) => new Vector3(left.x * right.x, left.y * right.y, left.z * right.z);
    public static Vector3 operator /(Vector3 left, Vector3 right) => new Vector3(left.x / right.x, left.y / right.y, left.z / right.z);
    public static Vector3 operator %(Vector3 left, Vector3 right) => new Vector3(left.x % right.x, left.y % right.y, left.z % right.z);

    public static Vector3 operator +(Vector3 left, float right) => new Vector3(left.x + right, left.y + right, left.z + right);
    public static Vector3 operator -(Vector3 left, float right) => new Vector3(left.x - right, left.y - right, left.z - right);
    public static Vector3 operator *(Vector3 left, float right) => new Vector3(left.x * right, left.y * right, left.z * right);
    public static Vector3 operator /(Vector3 left, float right) => new Vector3(left.x / right, left.y / right, left.z / right);
    public static Vector3 operator %(Vector3 left, float right) => new Vector3(left.x % right, left.y % right, left.z % right);
}
