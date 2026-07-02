/**
 * MongoDB Shell 自动补全 for Monaco Editor
 * 提供 db.collection.method() 风格的智能提示
 */
import * as monaco from "monaco-editor";
import { MONGO_SNIPPETS, renderSnippet, type SnippetDef } from "./mongo-snippets";

const SNIPPET_COLL_PREFIX = "db.${COLL}.";

// ---- MongoDB 方法定义 ----

interface MethodDef {
  label: string;
  insertText: string;
  detail: string;
  documentation: string;
  kind: monaco.languages.CompletionItemKind;
}

const collectionMethods: MethodDef[] = [
  {
    label: "find",
    insertText: "find(${1:{\\}})",
    detail: "method mongo.Collection.find(query object) : mongo.Cursor",
    documentation:
      "Selects documents in a collection and returns a cursor to the selected documents.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "findOne",
    insertText: "findOne(${1:{\\}})",
    detail: "method mongo.Collection.findOne(query object) : object",
    documentation:
      "Returns one document that satisfies the specified query criteria. If multiple documents match, this method returns the first document according to the natural order.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "aggregate",
    insertText: "aggregate(${1:[]})",
    detail: "method mongo.Collection.aggregate(pipeline object[]) : mongo.AggregationCursor",
    documentation:
      "Calculates aggregate values for the data in a collection. Use aggregate for aggregation pipeline operations, or if you include the explain option.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "countDocuments",
    insertText: "countDocuments(${1:{\\}})",
    detail: "method mongo.Collection.countDocuments(query object) : number",
    documentation: "Returns the count of documents that match the query for a collection or view.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "estimatedDocumentCount",
    insertText: "estimatedDocumentCount()",
    detail: "method mongo.Collection.estimatedDocumentCount() : number",
    documentation:
      "Returns the count of all documents in a collection or view based on collection metadata.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "distinct",
    insertText: 'distinct("${1:field}", ${2:{\\}})',
    detail: "method mongo.Collection.distinct(field string, query object) : array",
    documentation:
      "Finds the distinct values for a specified field across a single collection and returns the results in an array.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "insertOne",
    insertText: "insertOne(${1:{\\}})",
    detail: "method mongo.Collection.insertOne(document object) : InsertOneResult",
    documentation: "Inserts a single document into a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "insertMany",
    insertText: "insertMany(${1:[]})",
    detail: "method mongo.Collection.insertMany(documents object[]) : InsertManyResult",
    documentation: "Inserts multiple documents into a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "updateOne",
    insertText: "updateOne(${1:{\\}}, ${2:{\\$set: {\\}}})",
    detail:
      "method mongo.Collection.updateOne(filter object, update object, options?) : UpdateResult",
    documentation: "Updates a single document within the collection based on the filter.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "updateMany",
    insertText: "updateMany(${1:{\\}}, ${2:{\\$set: {\\}}})",
    detail:
      "method mongo.Collection.updateMany(filter object, update object, options?) : UpdateResult",
    documentation: "Updates all documents that match the specified filter for a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "replaceOne",
    insertText: "replaceOne(${1:{\\}}, ${2:{\\}})",
    detail: "method mongo.Collection.replaceOne(filter object, replacement object) : UpdateResult",
    documentation: "Replaces a single document within the collection based on the filter.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "deleteOne",
    insertText: "deleteOne(${1:{\\}})",
    detail: "method mongo.Collection.deleteOne(filter object) : DeleteResult",
    documentation: "Removes a single document from a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "deleteMany",
    insertText: "deleteMany(${1:{\\}})",
    detail: "method mongo.Collection.deleteMany(filter object) : DeleteResult",
    documentation: "Removes all documents that match the filter from a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "bulkWrite",
    insertText: "bulkWrite(${1:[]})",
    detail: "method mongo.Collection.bulkWrite(operations object[]) : BulkWriteResult",
    documentation: "Performs multiple write operations with controls for order of execution.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "findOneAndUpdate",
    insertText: "findOneAndUpdate(${1:{\\}}, ${2:{\\$set: {\\}}})",
    detail: "method mongo.Collection.findOneAndUpdate(filter, update, options?) : object",
    documentation:
      "Modifies and returns a single document. By default, the returned document does not include the modifications made on the update.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "findOneAndReplace",
    insertText: "findOneAndReplace(${1:{\\}}, ${2:{\\}})",
    detail: "method mongo.Collection.findOneAndReplace(filter, replacement, options?) : object",
    documentation: "Replaces a single document based on the specified filter.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "findOneAndDelete",
    insertText: "findOneAndDelete(${1:{\\}})",
    detail: "method mongo.Collection.findOneAndDelete(filter object, options?) : object",
    documentation:
      "Deletes a single document based on the filter and sort criteria, returning the deleted document.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "createIndex",
    insertText: 'createIndex(${1:{\\}}, ${2:{name: "${3:indexName}"\\}})',
    detail: "method mongo.Collection.createIndex(keys object, options?) : string",
    documentation:
      "Creates an index on the specified field(s) if the index does not already exist.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "createIndexes",
    insertText: "createIndexes(${1:[]})",
    detail: "method mongo.Collection.createIndexes(indexSpecs object[]) : string[]",
    documentation: "Creates one or more indexes on a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "dropIndex",
    insertText: 'dropIndex("${1:indexName}")',
    detail: "method mongo.Collection.dropIndex(index string|object) : object",
    documentation: "Drops the specified index from a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "dropIndexes",
    insertText: "dropIndexes()",
    detail: "method mongo.Collection.dropIndexes() : object",
    documentation: "Drops all indexes other than the required index on the _id field.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "getIndexes",
    insertText: "getIndexes()",
    detail: "method mongo.Collection.getIndexes() : object[]",
    documentation:
      "Returns an array that holds a list of documents that identify and describe the existing indexes on the collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "drop",
    insertText: "drop()",
    detail: "method mongo.Collection.drop() : boolean",
    documentation:
      "Removes a collection from the database. The method also removes any indexes associated with the dropped collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "renameCollection",
    insertText: 'renameCollection("${1:newName}")',
    detail: "method mongo.Collection.renameCollection(newName string) : object",
    documentation: "Renames a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "stats",
    insertText: "stats()",
    detail: "method mongo.Collection.stats() : object",
    documentation: "Returns statistics about the collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "dataSize",
    insertText: "dataSize()",
    detail: "method mongo.Collection.dataSize() : number",
    documentation: "Returns the size of the collection's data.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "storageSize",
    insertText: "storageSize()",
    detail: "method mongo.Collection.storageSize() : number",
    documentation:
      "Returns the total size in bytes of the data in the collection plus the size of every index.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "totalSize",
    insertText: "totalSize()",
    detail: "method mongo.Collection.totalSize() : number",
    documentation: "Returns the total size of the collection including indexes.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "watch",
    insertText: "watch(${1:[]})",
    detail: "method mongo.Collection.watch(pipeline? object[]) : ChangeStream",
    documentation: "Opens a change stream cursor on a collection.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
];

const cursorMethods: MethodDef[] = [
  {
    label: "sort",
    insertText: "sort(${1:{_id: 1\\}})",
    detail: "method mongo.Cursor.sort(sortSpec object) : mongo.Cursor",
    documentation:
      "Specifies the order in which the query returns matching documents. {field: 1} for ascending, {field: -1} for descending.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "limit",
    insertText: "limit(${1:20})",
    detail: "method mongo.Cursor.limit(n number) : mongo.Cursor",
    documentation: "Specifies the maximum number of documents the cursor will return.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "skip",
    insertText: "skip(${1:0})",
    detail: "method mongo.Cursor.skip(n number) : mongo.Cursor",
    documentation:
      "Controls where MongoDB begins returning results. Used for pagination together with limit().",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "projection",
    insertText: "projection(${1:{\\}})",
    detail: "method mongo.Cursor.projection(projectionSpec object) : mongo.Cursor",
    documentation:
      "Specifies which fields to include or exclude. {field: 1} to include, {field: 0} to exclude.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "count",
    insertText: "count()",
    detail: "method mongo.Cursor.count() : number",
    documentation: "Returns the total number of documents that match the query.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "toArray",
    insertText: "toArray()",
    detail: "method mongo.Cursor.toArray() : object[]",
    documentation: "Returns an array that contains all documents returned by the cursor.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "forEach",
    insertText: "forEach(${1:function(doc) {\\}})",
    detail: "method mongo.Cursor.forEach(function) : void",
    documentation:
      "Iterates the cursor to apply a JavaScript function to each document from the cursor.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "map",
    insertText: "map(${1:function(doc) { return doc; \\}})",
    detail: "method mongo.Cursor.map(function) : array",
    documentation:
      "Applies a function to each document visited by the cursor and returns an array of the mapped values.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "hasNext",
    insertText: "hasNext()",
    detail: "method mongo.Cursor.hasNext() : boolean",
    documentation: "Returns true if the cursor has documents and can be iterated.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "next",
    insertText: "next()",
    detail: "method mongo.Cursor.next() : object",
    documentation: "Returns the next document in a cursor.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "pretty",
    insertText: "pretty()",
    detail: "method mongo.Cursor.pretty() : mongo.Cursor",
    documentation: "Configures the cursor to display results in an easy-to-read format.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "explain",
    insertText: 'explain("${1:executionStats}")',
    detail: "method mongo.Cursor.explain(verbosity? string) : object",
    documentation:
      'Provides information on the query plan. Verbosity: "queryPlanner", "executionStats", "allPlansExecution".',
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "hint",
    insertText: "hint(${1:{\\}})",
    detail: "method mongo.Cursor.hint(index object|string) : mongo.Cursor",
    documentation: "Forces MongoDB to use a specific index for a query.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "maxTimeMS",
    insertText: "maxTimeMS(${1:5000})",
    detail: "method mongo.Cursor.maxTimeMS(ms number) : mongo.Cursor",
    documentation:
      "Specifies a cumulative time limit in milliseconds for processing operations on the cursor.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "batchSize",
    insertText: "batchSize(${1:20})",
    detail: "method mongo.Cursor.batchSize(size number) : mongo.Cursor",
    documentation:
      "Specifies the number of documents to return in each batch of the response from the MongoDB instance.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "collation",
    insertText: 'collation(${1:{locale: "${2:en}"\\}})',
    detail: "method mongo.Cursor.collation(spec object) : mongo.Cursor",
    documentation:
      "Specifies the collation for the operation, which allows language-specific rules for string comparison.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
];

const dbMethods: MethodDef[] = [
  {
    label: "getCollection",
    insertText: 'getCollection("${1:collectionName}")',
    detail: "method mongo.Database.getCollection(name string) : mongo.Collection",
    documentation:
      "Returns a collection object that is functionally equivalent to using the db.<collectionName> syntax.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "getCollectionNames",
    insertText: "getCollectionNames()",
    detail: "method mongo.Database.getCollectionNames() : string[]",
    documentation:
      "Returns an array containing the names of all collections in the current database.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "createCollection",
    insertText: 'createCollection("${1:name}", ${2:{\\}})',
    detail: "method mongo.Database.createCollection(name string, options?) : object",
    documentation: "Creates a new collection explicitly.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "dropDatabase",
    insertText: "dropDatabase()",
    detail: "method mongo.Database.dropDatabase() : object",
    documentation: "Removes the current database, deleting the associated data files.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "stats",
    insertText: "stats()",
    detail: "method mongo.Database.stats() : object",
    documentation: "Returns statistics that reflect the use state of a single database.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "runCommand",
    insertText: "runCommand(${1:{\\}})",
    detail: "method mongo.Database.runCommand(command object) : object",
    documentation:
      "Provides a helper to run specified database commands against the current database.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "adminCommand",
    insertText: "adminCommand(${1:{\\}})",
    detail: "method mongo.Database.adminCommand(command object) : object",
    documentation: "Runs a database command against the admin database.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "getMongo",
    insertText: "getMongo()",
    detail: "method mongo.Database.getMongo() : Mongo",
    documentation: "Returns the Mongo connection object for the current session.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "getName",
    insertText: "getName()",
    detail: "method mongo.Database.getName() : string",
    documentation: "Returns the name of the current database.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "currentOp",
    insertText: "currentOp()",
    detail: "method mongo.Database.currentOp() : object",
    documentation:
      "Returns a document that contains information on in-progress operations for the database instance.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "serverStatus",
    insertText: "serverStatus()",
    detail: "method mongo.Database.serverStatus() : object",
    documentation: "Returns a document that provides an overview of the database process's state.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
  {
    label: "version",
    insertText: "version()",
    detail: "method mongo.Database.version() : string",
    documentation: "Returns the version of the MongoDB instance.",
    kind: monaco.languages.CompletionItemKind.Method,
  },
];

// ---- 全局函数 / BSON 构造器 / mongosh 内置 ----

const globalFunctions: MethodDef[] = [
  {
    label: "ObjectId",
    insertText: 'ObjectId("${1}")',
    detail: "BSON ObjectId(hexString?)",
    documentation: "构造一个 ObjectId。不传参生成新的。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "ISODate",
    insertText: 'ISODate("${1}")',
    detail: "ISODate(dateString?)",
    documentation: '构造一个日期。不传参为当前时间, 例如 ISODate("2025-01-01")。',
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "NumberLong",
    insertText: 'NumberLong("${1}")',
    detail: "NumberLong(value)",
    documentation: "64 位整数 (Int64)。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "NumberInt",
    insertText: "NumberInt(${1})",
    detail: "NumberInt(value)",
    documentation: "32 位整数 (Int32)。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "NumberDecimal",
    insertText: 'NumberDecimal("${1}")',
    detail: "NumberDecimal(value)",
    documentation: "高精度十进制数 (Decimal128)。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "Double",
    insertText: "Double(${1})",
    detail: "Double(value)",
    documentation: "双精度浮点数。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "UUID",
    insertText: 'UUID("${1}")',
    detail: "UUID(hexString?)",
    documentation: "构造 UUID。不传参生成随机 UUID。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "Timestamp",
    insertText: "Timestamp(${1:0}, ${2:0})",
    detail: "Timestamp(t, i)",
    documentation: "BSON 时间戳。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "Date",
    insertText: "new Date(${1})",
    detail: "new Date(...)",
    documentation: "JavaScript 日期对象。",
    kind: monaco.languages.CompletionItemKind.Constructor,
  },
  {
    label: "Buffer.from",
    insertText: 'Buffer.from("${1}", "${2:utf8}")',
    detail: "Buffer.from(input, encoding)",
    documentation: "构造 Buffer (脚本里做 base64 / hex 编解码常用)。",
    kind: monaco.languages.CompletionItemKind.Function,
  },
  {
    label: "print",
    insertText: "print(${1})",
    detail: "print(...args)",
    documentation: "打印输出到结果的 Console 区。",
    kind: monaco.languages.CompletionItemKind.Function,
  },
  {
    label: "printjson",
    insertText: "printjson(${1})",
    detail: "printjson(obj)",
    documentation: "以 JSON 格式打印对象到 Console 区。",
    kind: monaco.languages.CompletionItemKind.Function,
  },
  {
    label: "load",
    insertText: 'load("${1}")',
    detail: "load(path)",
    documentation: "引入外部脚本文件 (绝对路径) 或脚本库脚本 (文件夹/脚本名)。",
    kind: monaco.languages.CompletionItemKind.Function,
  },
];

// ---- Update 操作符 ----

const updateOperators: MethodDef[] = [
  {
    label: "$set",
    insertText: "\\$set: {${1}}",
    detail: "Update operator",
    documentation: "Sets the value of a field in a document.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$unset",
    insertText: "\\$unset: {${1}}",
    detail: "Update operator",
    documentation: "Removes the specified field from a document.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$inc",
    insertText: "\\$inc: {${1}}",
    detail: "Update operator",
    documentation: "Increments the value of the field by the specified amount.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$mul",
    insertText: "\\$mul: {${1}}",
    detail: "Update operator",
    documentation: "Multiplies the value of the field by the specified amount.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$rename",
    insertText: "\\$rename: {${1}}",
    detail: "Update operator",
    documentation: "Renames a field.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$min",
    insertText: "\\$min: {${1}}",
    detail: "Update operator",
    documentation: "Only updates if the specified value is less than the existing field value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$max",
    insertText: "\\$max: {${1}}",
    detail: "Update operator",
    documentation: "Only updates if the specified value is greater than the existing field value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$push",
    insertText: "\\$push: {${1}}",
    detail: "Array update operator",
    documentation: "Adds an item to an array.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$pull",
    insertText: "\\$pull: {${1}}",
    detail: "Array update operator",
    documentation: "Removes all array elements that match a specified query.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$addToSet",
    insertText: "\\$addToSet: {${1}}",
    detail: "Array update operator",
    documentation: "Adds elements to an array only if they do not already exist in the set.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$pop",
    insertText: "\\$pop: {${1}}",
    detail: "Array update operator",
    documentation: "Removes the first or last item of an array. Pass -1 for first, 1 for last.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$currentDate",
    insertText: "\\$currentDate: {${1}}",
    detail: "Update operator",
    documentation: "Sets the value of a field to current date.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
];

// ---- Aggregation stages ----

const aggregationStages: MethodDef[] = [
  {
    label: "$match",
    insertText: "\\$match: {${1}}",
    detail: "Aggregation stage",
    documentation: "Filters documents to pass only those that match the specified condition(s).",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$group",
    insertText: "\\$group: { _id: ${1}, ${2} }",
    detail: "Aggregation stage",
    documentation:
      "Groups input documents by a specified identifier expression and applies accumulator expressions.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$project",
    insertText: "\\$project: {${1}}",
    detail: "Aggregation stage",
    documentation: "Reshapes each document by adding new fields or removing existing fields.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$sort",
    insertText: "\\$sort: {${1}: ${2:1}}",
    detail: "Aggregation stage",
    documentation:
      "Reorders the document stream by a specified sort key. 1 for ascending, -1 for descending.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$limit",
    insertText: "\\$limit: ${1:10}",
    detail: "Aggregation stage",
    documentation: "Restricts the number of documents passed to the next stage.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$skip",
    insertText: "\\$skip: ${1:0}",
    detail: "Aggregation stage",
    documentation: "Skips the specified number of documents that pass into the stage.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$unwind",
    insertText: '\\$unwind: "${1:$arrayField}"',
    detail: "Aggregation stage",
    documentation: "Deconstructs an array field to output a document for each element.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$lookup",
    insertText: '\\$lookup: { from: "${1}", localField: "${2}", foreignField: "${3}", as: "${4}" }',
    detail: "Aggregation stage",
    documentation: "Performs a left outer join to another collection in the same database.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$addFields",
    insertText: "\\$addFields: {${1}}",
    detail: "Aggregation stage",
    documentation: "Adds new fields to documents.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$count",
    insertText: '\\$count: "${1:count}"',
    detail: "Aggregation stage",
    documentation: "Passes a document with the count of documents to the next stage.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$facet",
    insertText: "\\$facet: {${1}}",
    detail: "Aggregation stage",
    documentation:
      "Processes multiple aggregation pipelines within a single stage on the same set of input documents.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$bucket",
    insertText: "\\$bucket: { groupBy: ${1}, boundaries: [${2}], default: ${3} }",
    detail: "Aggregation stage",
    documentation: "Categorizes documents into groups (buckets) based on a specified expression.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$out",
    insertText: '\\$out: "${1:collectionName}"',
    detail: "Aggregation stage",
    documentation: "Writes the resulting documents to a collection. Must be the last stage.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$merge",
    insertText: '\\$merge: { into: "${1}" }',
    detail: "Aggregation stage",
    documentation: "Writes the results to a specified collection. Can merge instead of replace.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$replaceRoot",
    insertText: "\\$replaceRoot: { newRoot: ${1} }",
    detail: "Aggregation stage",
    documentation: "Replaces the input document with the specified document.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$sample",
    insertText: "\\$sample: { size: ${1:10} }",
    detail: "Aggregation stage",
    documentation: "Randomly selects the specified number of documents.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
];

// ---- Query 操作符 ----

const queryOperators: MethodDef[] = [
  {
    label: "$eq",
    insertText: "\\$eq: ${1}",
    detail: "Comparison",
    documentation: "Matches values that are equal to a specified value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$ne",
    insertText: "\\$ne: ${1}",
    detail: "Comparison",
    documentation: "Matches all values that are not equal to a specified value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$gt",
    insertText: "\\$gt: ${1}",
    detail: "Comparison",
    documentation: "Matches values that are greater than a specified value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$gte",
    insertText: "\\$gte: ${1}",
    detail: "Comparison",
    documentation: "Matches values that are greater than or equal to a specified value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$lt",
    insertText: "\\$lt: ${1}",
    detail: "Comparison",
    documentation: "Matches values that are less than a specified value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$lte",
    insertText: "\\$lte: ${1}",
    detail: "Comparison",
    documentation: "Matches values that are less than or equal to a specified value.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$in",
    insertText: "\\$in: [${1}]",
    detail: "Comparison",
    documentation: "Matches any of the values specified in an array.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$nin",
    insertText: "\\$nin: [${1}]",
    detail: "Comparison",
    documentation: "Matches none of the values specified in an array.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$and",
    insertText: "\\$and: [${1}]",
    detail: "Logical",
    documentation: "Joins query clauses with a logical AND.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$or",
    insertText: "\\$or: [${1}]",
    detail: "Logical",
    documentation: "Joins query clauses with a logical OR.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$not",
    insertText: "\\$not: {${1}}",
    detail: "Logical",
    documentation: "Inverts the effect of a query expression.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$nor",
    insertText: "\\$nor: [${1}]",
    detail: "Logical",
    documentation: "Joins query clauses with a logical NOR.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$exists",
    insertText: "\\$exists: ${1:true}",
    detail: "Element",
    documentation: "Matches documents that have the specified field.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$type",
    insertText: '\\$type: "${1}"',
    detail: "Element",
    documentation: "Selects documents if a field is of the specified type.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$regex",
    insertText: '\\$regex: "${1}"',
    detail: "Evaluation",
    documentation: "Selects documents where values match a specified regular expression.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$expr",
    insertText: "\\$expr: {${1}}",
    detail: "Evaluation",
    documentation: "Allows use of aggregation expressions within the query language.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$elemMatch",
    insertText: "\\$elemMatch: {${1}}",
    detail: "Array",
    documentation:
      "Matches documents that contain an array field with at least one element that matches all specified query criteria.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$size",
    insertText: "\\$size: ${1}",
    detail: "Array",
    documentation: "Selects documents if the array field is a specified size.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
  {
    label: "$all",
    insertText: "\\$all: [${1}]",
    detail: "Array",
    documentation: "Matches arrays that contain all specified elements.",
    kind: monaco.languages.CompletionItemKind.Property,
  },
];

// ---- 判断上下文 ----

/** 检查文本是否在 .find() 等返回 cursor 的方法链后 */
function isCursorContext(textBefore: string): boolean {
  return /\.\s*(find|aggregate)\s*\([^)]*\)\s*(\.\s*\w+\s*\([^)]*\)\s*)*\.\s*$/.test(textBefore);
}

/** 检查是否在 db.xxx. 上下文（集合方法） */
function isCollectionContext(textBefore: string): boolean {
  return /db\s*\.\s*(?:getCollection\s*\(\s*["'][^"']*["']\s*\)|[a-zA-Z_$][a-zA-Z0-9_$]*)\s*\.\s*$/.test(
    textBefore,
  );
}

/** 检查是否在 db. 后面 */
function isDbContext(textBefore: string): boolean {
  return /db\s*\.\s*$/.test(textBefore);
}

const NON_COLLECTION_DB_METHODS = new Set([
  "getCollection",
  "getCollectionNames",
  "createCollection",
  "dropDatabase",
  "stats",
  "runCommand",
  "adminCommand",
  "getMongo",
  "getName",
  "currentOp",
  "serverStatus",
  "version",
]);

/** 从查询文本中提取集合名。
 *  text 是「文档开头 → 光标」整段, 编辑器里往往有多条语句;
 *  必须取光标前最近 (最后) 一次 db.xxx 引用, 否则永远命中第一条语句的集合. */
function extractCollectionName(text: string): string | null {
  let best: { index: number; name: string } | null = null;
  const consider = (re: RegExp, excludeDbMethods = false) => {
    for (const m of text.matchAll(re)) {
      const name = m[1];
      if (excludeDbMethods && NON_COLLECTION_DB_METHODS.has(name)) continue;
      const index = m.index ?? -1;
      if (!best || index >= best.index) best = { index, name };
    }
  };
  // db.getCollection("name")
  consider(/db\s*\.\s*getCollection\s*\(\s*["']([^"']+)["']\s*\)/g);
  // db.collName.method(
  consider(
    /db\s*\.\s*([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\.\s*(?:find|findOne|aggregate|countDocuments|distinct|insertOne|insertMany|updateOne|updateMany|deleteOne|deleteMany|findOneAndUpdate|findOneAndReplace|findOneAndDelete|bulkWrite|replaceOne)\s*\(/g,
  );
  // 简单匹配 db.xxx.
  consider(/db\s*\.\s*([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\./g, true);
  return best ? (best as { index: number; name: string }).name : null;
}

/** 检查光标是否在 {} 内部（MongoDB 查询/投影/排序上下文） */
function isInsideBraces(text: string): boolean {
  let depth = 0;
  for (let i = text.length - 1; i >= 0; i--) {
    const ch = text[i];
    if (ch === "}") depth++;
    else if (ch === "{") {
      if (depth === 0) return true;
      depth--;
    }
  }
  return false;
}

/** 检查光标是否在字符串引号内 */
function isInsideQuotes(text: string): boolean {
  // 检查行内最后一个引号状态
  const lineStart = text.lastIndexOf("\n") + 1;
  const line = text.slice(lineStart);
  let inStr = false;
  let quoteChar = "";
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (!inStr && (ch === '"' || ch === "'")) {
      inStr = true;
      quoteChar = ch;
    } else if (inStr && ch === quoteChar && line[i - 1] !== "\\") {
      inStr = false;
    }
  }
  return inStr;
}

// ---- 字段信息 ----

export interface FieldCompletionInfo {
  name: string;
  types: string; // e.g. "String", "Int32, String"
  collection: string;
}

// ---- 注册补全 ----

/** 已注册的 provider disposable —— HMR / 重复 register 时先 dispose 旧的, 避免重复触发 */
let _disposables: monaco.IDisposable[] = [];

export interface CompletionOptions {
  collectionNames?: () => string[];
  getFieldNames?: (collection: string) => Promise<FieldCompletionInfo[]>;
  /** 当前 tab 绑定的集合名 (片段补全里渲染 ${COLL} 占位) */
  currentCollection?: () => string;
}

/**
 * 把片段转成 Monaco CompletionItem (snippet kind).
 *
 * sortText 策略: "<method> <label>" —— 让 "find — 条件查询" 排在方法 "find" 之后、
 * 方法 "findOne" 之前 (lex 比较: "find" < "find …" < "findOne", 因为 ' ' (32) < 'O' (79)).
 * 这样相同关键字的方法 + 片段在补全列表里挨着出现.
 */
function snippetToItem(
  snip: SnippetDef,
  body: string,
  range: monaco.IRange,
): monaco.languages.CompletionItem {
  const dashIdx = snip.label.indexOf(" — ");
  const methodKey = dashIdx > 0 ? snip.label.slice(0, dashIdx).trim() : snip.label;
  return {
    label: snip.label,
    kind: monaco.languages.CompletionItemKind.Snippet,
    insertText: body,
    detail: `snippet — ${snip.group}`,
    documentation: { value: `**${snip.desc}**\n\n\`\`\`js\n${body}\n\`\`\`` },
    range,
    sortText: `${methodKey} ${snip.label}`,
  };
}

export function registerMongoCompletions(options?: CompletionOptions): void;
export function registerMongoCompletions(collectionNames?: () => string[]): void;
export function registerMongoCompletions(arg?: CompletionOptions | (() => string[])): void {
  // 每次调用先 dispose 上一次的 provider, 防止 HMR / 多次注册导致补全 widget 行为异常
  // (重复 provider 会让同一项出现多次, 或在某些 Monaco 状态下完全不弹).
  for (const d of _disposables) {
    try {
      d.dispose();
    } catch {
      /* ignore */
    }
  }
  _disposables = [];

  const opts: CompletionOptions =
    typeof arg === "function" ? { collectionNames: arg } : (arg ?? {});

  const provider: monaco.languages.CompletionItemProvider = {
    triggerCharacters: [".", "$", '"', "'", "{"],
    provideCompletionItems: async (model, position) => {
      try {
        return await provideCompletionsImpl(model, position);
      } catch (err) {
        // 一旦 provider 抛错, Monaco 会把它"标黑"在本次会话剩余时间不再调用 -> 用户感觉补全完全失效.
        // 抓住所有错误, 至少返回空 suggestions 保住 provider 活着, 同时把堆栈打到 DevTools.
        console.error("[mongo-completions] provideCompletionItems failed", err);
        return { suggestions: [] };
      }
    },
  };

  async function provideCompletionsImpl(
    model: monaco.editor.ITextModel,
    position: monaco.Position,
  ): Promise<monaco.languages.CompletionList> {
    const textUntilPosition = model.getValueInRange({
      startLineNumber: 1,
      startColumn: 1,
      endLineNumber: position.lineNumber,
      endColumn: position.column,
    });

    const word = model.getWordUntilPosition(position);
    const range: monaco.IRange = {
      startLineNumber: position.lineNumber,
      startColumn: word.startColumn,
      endLineNumber: position.lineNumber,
      endColumn: word.endColumn,
    };

    const suggestions: monaco.languages.CompletionItem[] = [];

    // 1. cursor chain context: .find({}).| or .aggregate([]).sort().|
    if (isCursorContext(textUntilPosition)) {
      for (const m of cursorMethods) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range,
        });
      }
      for (const m of collectionMethods) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range,
          sortText: "z" + m.label,
        });
      }
      return { suggestions };
    }

    // 2. collection context: db.collName.| or db.getCollection("name").|
    if (isCollectionContext(textUntilPosition)) {
      for (const m of collectionMethods) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range,
        });
      }
      // 片段: 只挑 `db.${COLL}.` 开头的, 剥掉前缀后插入 (用户输入处已经有 db.coll.)
      for (const snip of MONGO_SNIPPETS) {
        if (!snip.body.startsWith(SNIPPET_COLL_PREFIX)) continue;
        const body = snip.body.slice(SNIPPET_COLL_PREFIX.length);
        suggestions.push(snippetToItem(snip, body, range));
      }
      return { suggestions };
    }

    // 3. db. context
    if (isDbContext(textUntilPosition)) {
      for (const m of dbMethods) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range,
        });
      }
      if (opts.collectionNames) {
        for (const name of opts.collectionNames()) {
          suggestions.push({
            label: name,
            kind: monaco.languages.CompletionItemKind.Field,
            insertText: name,
            detail: "Collection",
            documentation: { value: `Access collection \`${name}\`` },
            range,
          });
        }
      }
      return { suggestions };
    }

    // 4. $ 操作符 (inside {} context)
    const lineText = model.getLineContent(position.lineNumber);
    const charBefore = lineText.substring(0, position.column - 1);
    if (charBefore.trimEnd().endsWith("$") || /\$\w*$/.test(charBefore)) {
      const inAggregate = /\.aggregate\s*\(\s*\[/s.test(textUntilPosition);

      if (inAggregate) {
        for (const m of aggregationStages) {
          suggestions.push({
            label: m.label,
            kind: m.kind,
            insertText: m.insertText,
            insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
            detail: m.detail,
            documentation: { value: m.documentation },
            range: { ...range, startColumn: Math.max(1, range.startColumn - 1) },
          });
        }
      }

      for (const m of updateOperators) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range: { ...range, startColumn: Math.max(1, range.startColumn - 1) },
        });
      }

      for (const m of queryOperators) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range: { ...range, startColumn: Math.max(1, range.startColumn - 1) },
        });
      }

      // 也在 $ 上下文提供字段名（如 $set: { field: ... }）
      if (opts.getFieldNames) {
        const collName = extractCollectionName(textUntilPosition);
        if (collName) {
          try {
            const fields = await opts.getFieldNames(collName);
            for (const f of fields) {
              suggestions.push({
                label: f.name,
                kind: monaco.languages.CompletionItemKind.Field,
                insertText: f.name,
                detail: f.collection,
                documentation: { value: `Field \`${f.name}\` (${f.types})` },
                range: { ...range, startColumn: Math.max(1, range.startColumn - 1) },
                sortText: "zz" + f.name, // lower priority than operators
              });
            }
          } catch {
            /* ignore schema fetch errors */
          }
        }
      }

      return { suggestions };
    }

    // 5. 字段名提示：光标在 {} 或 "" 内部
    const collName = extractCollectionName(textUntilPosition);
    if (collName && opts.getFieldNames) {
      const inBraces = isInsideBraces(textUntilPosition);
      const inQuotes = isInsideQuotes(textUntilPosition);

      if (inBraces || inQuotes) {
        try {
          const fields = await opts.getFieldNames(collName);

          // 如果在引号内，调整 range 覆盖引号内的文本
          let fieldRange = range;
          if (inQuotes) {
            // 找到引号起始位置
            const lineUpToCursor = lineText.substring(0, position.column - 1);
            const lastQuote = Math.max(
              lineUpToCursor.lastIndexOf('"'),
              lineUpToCursor.lastIndexOf("'"),
            );
            if (lastQuote >= 0) {
              fieldRange = {
                startLineNumber: position.lineNumber,
                startColumn: lastQuote + 2, // after the quote
                endLineNumber: position.lineNumber,
                endColumn: position.column,
              };
            }
          }

          for (const f of fields) {
            suggestions.push({
              label: f.name,
              kind: monaco.languages.CompletionItemKind.Field,
              insertText: f.name,
              detail: f.collection,
              documentation: { value: `Field \`${f.name}\` (${f.types})` },
              range: fieldRange,
            });
          }

          // 在 {} 内也提供 $ 操作符（低优先级）
          if (inBraces && !inQuotes) {
            for (const m of queryOperators) {
              suggestions.push({
                label: m.label,
                kind: m.kind,
                insertText: m.insertText,
                insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
                detail: m.detail,
                documentation: { value: m.documentation },
                range,
                sortText: "zz" + m.label,
              });
            }
          }
        } catch {
          /* ignore schema fetch errors */
        }

        return { suggestions };
      }
    }

    // 6. 兜底: 一般 JS 位置 (不在引号里) -> 提供全局函数 / BSON 构造器 / db / 代码片段
    if (!isInsideQuotes(textUntilPosition)) {
      for (const m of globalFunctions) {
        suggestions.push({
          label: m.label,
          kind: m.kind,
          insertText: m.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          detail: m.detail,
          documentation: { value: m.documentation },
          range,
        });
      }
      suggestions.push({
        label: "db",
        kind: monaco.languages.CompletionItemKind.Variable,
        insertText: "db",
        detail: "当前数据库",
        documentation: { value: "MongoDB 数据库句柄, db.<集合>.<方法>()" },
        range,
      });
      // 代码片段: 全量列出, ${COLL} 用当前 tab 的集合名 (没有就 collection 占位)
      const coll = opts.currentCollection?.() || "";
      for (const snip of MONGO_SNIPPETS) {
        const body = renderSnippet(snip.body, coll);
        suggestions.push(snippetToItem(snip, body, range));
      }
    }

    return { suggestions };
  }

  // 同时挂到 javascript 和 mongosh 两个 language id
  _disposables.push(
    monaco.languages.registerCompletionItemProvider("javascript", provider),
    monaco.languages.registerCompletionItemProvider("mongosh", provider),
  );
}
