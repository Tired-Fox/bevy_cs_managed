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
}
